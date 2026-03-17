use serde_json::Value;
use std::str::FromStr;
use tstring_syntax::{
    BackendError, BackendResult, NormalizedDocument, NormalizedFloat, NormalizedKey,
    NormalizedStream, NormalizedValue, SourcePosition, SourceSpan, StreamItem, TemplateInput,
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum JsonProfile {
    Rfc8259,
}

impl JsonProfile {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rfc8259 => "rfc8259",
        }
    }
}

impl Default for JsonProfile {
    fn default() -> Self {
        Self::Rfc8259
    }
}

impl FromStr for JsonProfile {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "rfc8259" => Ok(Self::Rfc8259),
            other => Err(format!(
                "Unsupported JSON profile {other:?}. Supported profiles: \"rfc8259\"."
            )),
        }
    }
}

#[derive(Clone, Debug)]
pub struct JsonInterpolationNode {
    pub span: SourceSpan,
    pub interpolation_index: usize,
    pub role: String,
}

#[derive(Clone, Debug)]
pub struct JsonStringChunkNode {
    pub span: SourceSpan,
    pub value: String,
}

#[derive(Clone, Debug)]
pub enum JsonStringPart {
    Chunk(JsonStringChunkNode),
    Interpolation(JsonInterpolationNode),
}

#[derive(Clone, Debug)]
pub struct JsonStringNode {
    pub span: SourceSpan,
    pub chunks: Vec<JsonStringPart>,
    pub quoted: bool,
}

#[derive(Clone, Debug)]
pub struct JsonLiteralNode {
    pub span: SourceSpan,
    pub source: String,
    pub value: Value,
}

#[derive(Clone, Debug)]
pub struct JsonKeyNode {
    pub span: SourceSpan,
    pub value: JsonKeyValue,
}

#[derive(Clone, Debug)]
pub enum JsonKeyValue {
    String(JsonStringNode),
    Interpolation(JsonInterpolationNode),
}

#[derive(Clone, Debug)]
pub struct JsonMemberNode {
    pub span: SourceSpan,
    pub key: JsonKeyNode,
    pub value: JsonValueNode,
}

#[derive(Clone, Debug)]
pub struct JsonObjectNode {
    pub span: SourceSpan,
    pub members: Vec<JsonMemberNode>,
}

#[derive(Clone, Debug)]
pub struct JsonArrayNode {
    pub span: SourceSpan,
    pub items: Vec<JsonValueNode>,
}

#[derive(Clone, Debug)]
pub struct JsonDocumentNode {
    pub span: SourceSpan,
    pub value: JsonValueNode,
}

#[derive(Clone, Debug)]
pub enum JsonValueNode {
    String(JsonStringNode),
    Literal(JsonLiteralNode),
    Interpolation(JsonInterpolationNode),
    Object(JsonObjectNode),
    Array(JsonArrayNode),
}

pub struct JsonParser {
    items: Vec<StreamItem>,
    index: usize,
}

impl JsonParser {
    #[must_use]
    pub fn new(template: &TemplateInput) -> Self {
        Self {
            items: template.flatten(),
            index: 0,
        }
    }

    pub fn parse(&mut self) -> BackendResult<JsonDocumentNode> {
        let start = self.mark();
        let value = self.parse_value()?;
        self.skip_whitespace();
        if self.current_kind() != "eof" {
            return Err(self.error("Unexpected trailing content in JSON template."));
        }
        Ok(JsonDocumentNode {
            span: self.span_from(start),
            value,
        })
    }

    fn current(&self) -> &StreamItem {
        &self.items[self.index]
    }

    fn current_kind(&self) -> &'static str {
        self.current().kind()
    }

    fn current_char(&self) -> Option<char> {
        self.current().char()
    }

    fn mark(&self) -> SourcePosition {
        self.current().span().start.clone()
    }

    fn previous_end(&self) -> SourcePosition {
        if self.index == 0 {
            return self.current().span().start.clone();
        }
        self.items[self.index - 1].span().end.clone()
    }

    fn span_from(&self, start: SourcePosition) -> SourceSpan {
        SourceSpan::between(start, self.previous_end())
    }

    fn error(&self, message: impl Into<String>) -> BackendError {
        BackendError::parse_at("json.parse", message, Some(self.current().span().clone()))
    }

    fn advance(&mut self) {
        if self.current_kind() != "eof" {
            self.index += 1;
        }
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.current_char(), Some(ch) if ch.is_whitespace()) {
            self.advance();
        }
    }

    fn consume_char(&mut self, expected: char) -> BackendResult<()> {
        if self.current_char() != Some(expected) {
            return Err(self.error(format!("Expected {expected:?} in JSON template.")));
        }
        self.advance();
        Ok(())
    }

    fn parse_value(&mut self) -> BackendResult<JsonValueNode> {
        self.skip_whitespace();
        let start = self.mark();

        if self.current_kind() == "interpolation" {
            let interpolation = self.consume_interpolation("value")?;
            if self.starts_value_terminator() {
                return Ok(JsonValueNode::Interpolation(interpolation));
            }
            return Ok(JsonValueNode::String(self.parse_promoted_string(
                start,
                vec![JsonStringPart::Interpolation(interpolation)],
            )?));
        }

        if self.current_char() == Some('{') {
            return Ok(JsonValueNode::Object(self.parse_object()?));
        }
        if self.current_char() == Some('[') {
            return Ok(JsonValueNode::Array(self.parse_array()?));
        }
        if self.current_char() == Some('"') {
            return Ok(JsonValueNode::String(self.parse_string(true)?));
        }
        if matches!(self.current_char(), Some('-' | '0'..='9')) {
            return Ok(JsonValueNode::Literal(self.parse_number()?));
        }

        if let Some(literal) = self.try_consume_literal("true", Value::Bool(true))? {
            return Ok(JsonValueNode::Literal(literal));
        }
        if let Some(literal) = self.try_consume_literal("false", Value::Bool(false))? {
            return Ok(JsonValueNode::Literal(literal));
        }
        if let Some(literal) = self.try_consume_literal("null", Value::Null)? {
            return Ok(JsonValueNode::Literal(literal));
        }

        Ok(JsonValueNode::String(
            self.parse_promoted_string(start, Vec::new())?,
        ))
    }

    fn try_consume_literal(
        &mut self,
        text: &str,
        value: Value,
    ) -> BackendResult<Option<JsonLiteralNode>> {
        let start_index = self.index;
        let start = self.mark();
        for expected in text.chars() {
            if self.current_char() != Some(expected) {
                self.index = start_index;
                return Ok(None);
            }
            self.advance();
        }
        if !self.starts_value_terminator() {
            self.index = start_index;
            return Ok(None);
        }
        Ok(Some(JsonLiteralNode {
            span: self.span_from(start),
            source: text.to_owned(),
            value,
        }))
    }

    fn parse_number(&mut self) -> BackendResult<JsonLiteralNode> {
        let start = self.mark();
        let mut source = String::new();
        while matches!(
            self.current_char(),
            Some('-' | '+' | '.' | 'e' | 'E' | '0'..='9')
        ) {
            source.push(self.current_char().unwrap_or_default());
            self.advance();
        }
        if source.is_empty() {
            return Err(self.error("Expected a JSON number."));
        }
        if !self.starts_value_terminator() {
            return Err(self.error("Invalid JSON number literal."));
        }
        let value: Value = serde_json::from_str(&source)
            .map_err(|err| self.error(format!("Invalid JSON number literal: {err}")))?;
        Ok(JsonLiteralNode {
            span: self.span_from(start),
            source,
            value,
        })
    }

    fn parse_object(&mut self) -> BackendResult<JsonObjectNode> {
        let start = self.mark();
        self.consume_char('{')?;
        self.skip_whitespace();
        let mut members = Vec::new();
        if self.current_char() == Some('}') {
            self.advance();
            return Ok(JsonObjectNode {
                span: self.span_from(start),
                members,
            });
        }

        loop {
            let member_start = self.mark();
            let key = self.parse_key()?;
            self.skip_whitespace();
            self.consume_char(':')?;
            let value = self.parse_value()?;
            members.push(JsonMemberNode {
                span: self.span_from(member_start),
                key,
                value,
            });
            self.skip_whitespace();
            if self.current_char() == Some('}') {
                self.advance();
                break;
            }
            self.consume_char(',')?;
            self.skip_whitespace();
        }

        Ok(JsonObjectNode {
            span: self.span_from(start),
            members,
        })
    }

    fn parse_key(&mut self) -> BackendResult<JsonKeyNode> {
        self.skip_whitespace();
        let start = self.mark();
        if self.current_kind() == "interpolation" {
            return Ok(JsonKeyNode {
                span: self.span_from(start),
                value: JsonKeyValue::Interpolation(self.consume_interpolation("key")?),
            });
        }
        if self.current_char() != Some('"') {
            return Err(self.error("JSON object keys must be quoted strings or interpolations."));
        }
        Ok(JsonKeyNode {
            span: self.span_from(start),
            value: JsonKeyValue::String(self.parse_string(true)?),
        })
    }

    fn parse_array(&mut self) -> BackendResult<JsonArrayNode> {
        let start = self.mark();
        self.consume_char('[')?;
        self.skip_whitespace();
        let mut items = Vec::new();
        if self.current_char() == Some(']') {
            self.advance();
            return Ok(JsonArrayNode {
                span: self.span_from(start),
                items,
            });
        }

        loop {
            items.push(self.parse_value()?);
            self.skip_whitespace();
            if self.current_char() == Some(']') {
                self.advance();
                break;
            }
            self.consume_char(',')?;
            self.skip_whitespace();
        }

        Ok(JsonArrayNode {
            span: self.span_from(start),
            items,
        })
    }

    fn parse_string(&mut self, quoted: bool) -> BackendResult<JsonStringNode> {
        let start = self.mark();
        if quoted {
            self.consume_char('"')?;
        }
        let mut chunks = Vec::new();
        let mut buffer = String::new();

        loop {
            if quoted && self.current_char() == Some('"') {
                self.advance();
                break;
            }
            if quoted && self.current_kind() == "eof" {
                return Err(self.error("Unterminated JSON string."));
            }
            if self.current_kind() == "interpolation" {
                self.flush_buffer(&mut buffer, &mut chunks);
                chunks.push(JsonStringPart::Interpolation(
                    self.consume_interpolation("string_fragment")?,
                ));
                continue;
            }
            if self.current_char().is_none() {
                break;
            }
            if quoted && self.current_char() == Some('\\') {
                buffer.push(self.parse_escape_sequence()?);
                continue;
            }
            if quoted && self.current_char().is_some_and(|ch| ch < ' ') {
                return Err(self.error("Control characters are not allowed in JSON strings."));
            }

            buffer.push(self.current_char().unwrap_or_default());
            self.advance();
            if !quoted && self.starts_value_terminator() {
                break;
            }
        }

        self.flush_buffer(&mut buffer, &mut chunks);
        Ok(JsonStringNode {
            span: self.span_from(start),
            chunks,
            quoted,
        })
    }

    fn parse_escape_sequence(&mut self) -> BackendResult<char> {
        self.consume_char('\\')?;
        let escape_char = self
            .current_char()
            .ok_or_else(|| self.error("Incomplete JSON escape sequence."))?;
        self.advance();
        let mapped = match escape_char {
            '"' => Some('"'),
            '\\' => Some('\\'),
            '/' => Some('/'),
            'b' => Some('\u{0008}'),
            'f' => Some('\u{000c}'),
            'n' => Some('\n'),
            'r' => Some('\r'),
            't' => Some('\t'),
            _ => None,
        };
        if let Some(value) = mapped {
            return Ok(value);
        }
        if escape_char == 'u' {
            let codepoint = self.parse_unicode_escape_value()?;
            if (0xD800..=0xDBFF).contains(&codepoint) {
                self.consume_char('\\')?;
                self.consume_char('u')?;
                let low = self.parse_unicode_escape_value()?;
                if !(0xDC00..=0xDFFF).contains(&low) {
                    return Err(self.error("Invalid JSON unicode escape."));
                }
                let combined = 0x10000 + (((codepoint - 0xD800) << 10) | (low - 0xDC00));
                return char::from_u32(combined)
                    .ok_or_else(|| self.error("Invalid JSON unicode escape."));
            }
            if (0xDC00..=0xDFFF).contains(&codepoint) {
                return Err(self.error("Invalid JSON unicode escape."));
            }
            return char::from_u32(codepoint)
                .ok_or_else(|| self.error("Invalid JSON unicode escape."));
        }
        Err(self.error("Invalid JSON escape sequence."))
    }

    fn parse_unicode_escape_value(&mut self) -> BackendResult<u32> {
        let digits = self.collect_exact_chars(4)?;
        u32::from_str_radix(&digits, 16).map_err(|_| self.error("Invalid JSON unicode escape."))
    }

    fn collect_exact_chars(&mut self, count: usize) -> BackendResult<String> {
        let mut digits = String::new();
        for _ in 0..count {
            let ch = self
                .current_char()
                .ok_or_else(|| self.error("Unexpected end of JSON escape sequence."))?;
            digits.push(ch);
            self.advance();
        }
        Ok(digits)
    }

    fn flush_buffer(&self, buffer: &mut String, chunks: &mut Vec<JsonStringPart>) {
        if buffer.is_empty() {
            return;
        }
        chunks.push(JsonStringPart::Chunk(JsonStringChunkNode {
            span: SourceSpan::point(0, 0),
            value: std::mem::take(buffer),
        }));
    }

    fn parse_promoted_string(
        &mut self,
        start: SourcePosition,
        mut chunks: Vec<JsonStringPart>,
    ) -> BackendResult<JsonStringNode> {
        if chunks.is_empty() && self.starts_value_terminator() {
            return Err(self.error("Expected a JSON value."));
        }

        let mut buffer = String::new();
        let mut saw_interpolation = chunks
            .iter()
            .any(|chunk| matches!(chunk, JsonStringPart::Interpolation(_)));

        while self.current_kind() != "eof" && !self.starts_value_terminator() {
            if self.current_kind() == "interpolation" {
                self.flush_buffer(&mut buffer, &mut chunks);
                chunks.push(JsonStringPart::Interpolation(
                    self.consume_interpolation("string_fragment")?,
                ));
                saw_interpolation = true;
                continue;
            }
            if matches!(self.current_char(), Some('"' | '{' | '[' | ':')) {
                return Err(self.error("Invalid promoted JSON fragment content."));
            }
            if let Some(ch) = self.current_char() {
                buffer.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        self.flush_buffer(&mut buffer, &mut chunks);
        if !saw_interpolation {
            return Err(self.error("Expected a JSON value."));
        }
        self.trim_trailing_fragment_whitespace(&mut chunks);
        Ok(JsonStringNode {
            span: self.span_from(start),
            chunks,
            quoted: false,
        })
    }

    fn trim_trailing_fragment_whitespace(&self, chunks: &mut Vec<JsonStringPart>) {
        while let Some(last) = chunks.last_mut() {
            let JsonStringPart::Chunk(last_chunk) = last else {
                return;
            };
            let trimmed = last_chunk.value.trim_end().to_owned();
            if trimmed.is_empty() {
                chunks.pop();
                continue;
            }
            last_chunk.value = trimmed;
            return;
        }
    }

    fn starts_value_terminator(&self) -> bool {
        let mut probe = self.index;
        while matches!(self.items[probe].char(), Some(ch) if ch.is_whitespace()) {
            probe += 1;
        }
        matches!(
            &self.items[probe],
            StreamItem::Eof { .. }
                | StreamItem::Char {
                    ch: ',' | ']' | '}',
                    ..
                }
        )
    }

    fn consume_interpolation(&mut self, role: &str) -> BackendResult<JsonInterpolationNode> {
        let (interpolation_index, span) = match self.current() {
            StreamItem::Interpolation {
                interpolation_index,
                span,
                ..
            } => (*interpolation_index, span.clone()),
            _ => return Err(self.error("Expected an interpolation.")),
        };
        self.advance();
        Ok(JsonInterpolationNode {
            span,
            interpolation_index,
            role: role.to_owned(),
        })
    }
}

pub fn parse_template_with_profile(
    template: &TemplateInput,
    _profile: JsonProfile,
) -> BackendResult<JsonDocumentNode> {
    // JSON only exposes RFC 8259 in this phase, but the parameter is kept so
    // the backend stays aligned with the public profile-aware API shape.
    JsonParser::new(template).parse()
}

pub fn parse_template(template: &TemplateInput) -> BackendResult<JsonDocumentNode> {
    parse_template_with_profile(template, JsonProfile::default())
}

pub fn normalize_document_with_profile(
    value: &Value,
    _profile: JsonProfile,
) -> BackendResult<NormalizedStream> {
    // Normalization is currently profile-invariant for JSON, but it remains
    // profile-aware so future variants do not require another API break.
    Ok(NormalizedStream::new(vec![NormalizedDocument::Value(
        normalize_value(value)?,
    )]))
}

pub fn normalize_document(value: &Value) -> BackendResult<NormalizedStream> {
    normalize_document_with_profile(value, JsonProfile::default())
}

pub fn normalize_value(value: &Value) -> BackendResult<NormalizedValue> {
    match value {
        Value::Null => Ok(NormalizedValue::Null),
        Value::Bool(value) => Ok(NormalizedValue::Bool(*value)),
        Value::String(value) => Ok(NormalizedValue::String(value.clone())),
        Value::Array(values) => values
            .iter()
            .map(normalize_value)
            .collect::<BackendResult<Vec<_>>>()
            .map(NormalizedValue::Sequence),
        Value::Object(values) => values
            .iter()
            .map(|(key, value)| {
                Ok(tstring_syntax::NormalizedEntry {
                    key: NormalizedKey::String(key.clone()),
                    value: normalize_value(value)?,
                })
            })
            .collect::<BackendResult<Vec<_>>>()
            .map(NormalizedValue::Mapping),
        Value::Number(number) => normalize_number(number),
    }
}

fn normalize_number(number: &serde_json::Number) -> BackendResult<NormalizedValue> {
    let source = number.to_string();
    if source.contains(['.', 'e', 'E']) {
        return source
            .parse::<f64>()
            .map(NormalizedFloat::finite)
            .map(NormalizedValue::Float)
            .map_err(|err| {
                BackendError::semantic(format!(
                    "Validated JSON number {source} could not be normalized as a finite float: {err}"
                ))
            });
    }

    source.parse().map(NormalizedValue::Integer).map_err(|err| {
        BackendError::semantic(format!(
            "Validated JSON number {source} could not be normalized as an exact integer: {err}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::{JsonKeyValue, JsonStringPart, JsonValueNode, parse_template};
    use pyo3::prelude::*;
    use serde_json::{Map, Number, Value, json};
    use tstring_pyo3_bindings::{extract_template, json::render_document};
    use tstring_syntax::{BackendError, BackendResult, ErrorKind};

    fn parse_rendered_json(text: &str) -> BackendResult<Value> {
        serde_json::from_str(text).map_err(|err| {
            BackendError::parse(format!(
                "Rendered JSON could not be reparsed during test verification: {err}"
            ))
        })
    }

    #[test]
    fn parses_json_structure() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nleft='prefix'\nright='suffix'\ntemplate=t'{{\"prefix-{left}\": {left}-{right}}}'\n"
                ),
                pyo3::ffi::c_str!("test_json.py"),
                pyo3::ffi::c_str!("test_json"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "json_t/json_t_str").unwrap();
            let document = parse_template(&template).unwrap();
            let JsonValueNode::Object(object) = document.value else {
                panic!("expected object");
            };
            assert_eq!(object.members.len(), 1);
            let JsonKeyValue::String(key) = &object.members[0].key.value else {
                panic!("expected interpolated string key");
            };
            assert_eq!(key.chunks.len(), 2);
            assert!(matches!(key.chunks[1], JsonStringPart::Interpolation(_)));
            let JsonValueNode::String(value) = &object.members[0].value else {
                panic!("expected promoted string value");
            };
            assert_eq!(value.chunks.len(), 3);
            assert!(matches!(value.chunks[0], JsonStringPart::Interpolation(_)));
        });
    }

    #[test]
    fn renders_nested_collections_and_validates() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "items=[1, {'name': 'Ada'}]\nname='Ada'\ntemplate=t'{{\"items\": {items}, \"message\": \"hi-{name}\"}}'\n"
                ),
                pyo3::ffi::c_str!("test_json_render.py"),
                pyo3::ffi::c_str!("test_json_render"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "json_t/json_t_str").unwrap();
            let document = parse_template(&template).unwrap();
            let rendered = render_document(py, &document).unwrap();

            assert_eq!(rendered.data["message"], Value::String("hi-Ada".to_owned()));
            assert_eq!(rendered.data["items"][0], Value::Number(Number::from(1)));
            assert_eq!(
                rendered.data["items"][1]["name"],
                Value::String("Ada".to_owned())
            );
        });
    }

    #[test]
    fn renders_top_level_scalar_text() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "template=t'{1}'\nbool_template=t'{True}'\nnull_template=t'{None}'\n"
                ),
                pyo3::ffi::c_str!("test_json_scalar.py"),
                pyo3::ffi::c_str!("test_json_scalar"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "json_t/json_t_str").unwrap();
            let document = parse_template(&template).unwrap();
            let rendered = render_document(py, &document).unwrap();

            assert_eq!(rendered.text, "1");
            assert_eq!(rendered.data, Value::Number(Number::from(1)));

            let bool_template = module.getattr("bool_template").unwrap();
            let bool_template = extract_template(py, &bool_template, "json_t/json_t_str").unwrap();
            let document = parse_template(&bool_template).unwrap();
            let rendered = render_document(py, &document).unwrap();
            assert_eq!(rendered.text, "true");
            assert_eq!(rendered.data, Value::Bool(true));

            let null_template = module.getattr("null_template").unwrap();
            let null_template = extract_template(py, &null_template, "json_t/json_t_str").unwrap();
            let document = parse_template(&null_template).unwrap();
            let rendered = render_document(py, &document).unwrap();
            assert_eq!(rendered.text, "null");
            assert_eq!(rendered.data, Value::Null);
        });
    }

    #[test]
    fn renders_quoted_key_fragments_and_promoted_fragments() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "left='prefix'\nright='suffix'\nrows=[{'name': 'one'}, {'name': 'two'}]\nquoted_key=t'{{\"{left}-{right}\": {1}, \"value\": {True}}}'\nfragment=t'{{\"label\": {left}-{right}}}'\npromoted=t'{rows}'\n"
                ),
                pyo3::ffi::c_str!("test_json_fragments.py"),
                pyo3::ffi::c_str!("test_json_fragments"),
            )
            .unwrap();

            let quoted_key = module.getattr("quoted_key").unwrap();
            let quoted_key = extract_template(py, &quoted_key, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&quoted_key).unwrap()).unwrap();
            assert_eq!(rendered.data, json!({"prefix-suffix": 1, "value": true}));

            let fragment = module.getattr("fragment").unwrap();
            let fragment = extract_template(py, &fragment, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&fragment).unwrap()).unwrap();
            assert_eq!(rendered.text, "{\"label\": \"prefix-suffix\"}");
            assert_eq!(rendered.data, json!({"label": "prefix-suffix"}));

            let promoted = module.getattr("promoted").unwrap();
            let promoted = extract_template(py, &promoted, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&promoted).unwrap()).unwrap();
            assert_eq!(rendered.text, r#"[{"name": "one"}, {"name": "two"}]"#);
            assert_eq!(rendered.data, json!([{"name": "one"}, {"name": "two"}]));
        });
    }

    #[test]
    fn rejects_non_string_key_interpolation() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("key=1\ntemplate=t'{{{key}: 1}}'\n"),
                pyo3::ffi::c_str!("test_json_error.py"),
                pyo3::ffi::c_str!("test_json_error"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "json_t/json_t_str").unwrap();
            let document = parse_template(&template).unwrap();
            let err = match render_document(py, &document) {
                Ok(_) => panic!("expected JSON render failure"),
                Err(err) => err,
            };

            assert_eq!(err.kind, ErrorKind::Unrepresentable);
            assert!(err.message.contains("JSON object keys must be str"));
        });
    }

    #[test]
    fn rejects_unrepresentable_render_value_families() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "import math\nbad_map={1: 'x'}\nbad_nan=math.nan\nbad_set={1, 2}\nmap_template=t'{bad_map}'\nnan_template=t'{bad_nan}'\nset_template=t'{bad_set}'\n"
                ),
                pyo3::ffi::c_str!("test_json_unrepresentable.py"),
                pyo3::ffi::c_str!("test_json_unrepresentable"),
            )
            .unwrap();

            for (name, expected) in [
                ("map_template", "JSON object keys must be str"),
                ("nan_template", "non-finite float"),
                ("set_template", "could not be rendered as JSON"),
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "json_t/json_t_str").unwrap();
                let document = parse_template(&template).unwrap();
                let err = match render_document(py, &document) {
                    Ok(_) => panic!("expected JSON render failure"),
                    Err(err) => err,
                };
                assert_eq!(err.kind, ErrorKind::Unrepresentable);
                assert!(err.message.contains(expected), "{name}: {}", err.message);
            }
        });
    }

    #[test]
    fn parses_unicode_surrogate_pairs() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("template=t'\"\\\\uD834\\\\uDD1E\"'\n"),
                pyo3::ffi::c_str!("test_json_unicode.py"),
                pyo3::ffi::c_str!("test_json_unicode"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "json_t/json_t_str").unwrap();
            let document = parse_template(&template).unwrap();
            let rendered = render_document(py, &document).unwrap();

            assert_eq!(rendered.data, Value::String("𝄞".to_owned()));
        });
    }

    #[test]
    fn parses_numbers_and_escape_sequences() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "template=t'{{\"int\": -0, \"exp\": 1.5e2, \"escapes\": \"\\\\b\\\\f\\\\n\\\\r\\\\t\\\\/\", \"unicode\": \"\\\\u00DF\\\\u6771\\\\uD834\\\\uDD1E\"}}'\n"
                ),
                pyo3::ffi::c_str!("test_json_numbers.py"),
                pyo3::ffi::c_str!("test_json_numbers"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "json_t/json_t_str").unwrap();
            let document = parse_template(&template).unwrap();
            let rendered = render_document(py, &document).unwrap();

            assert_eq!(
                rendered.data["exp"],
                Value::Number(Number::from_f64(150.0).unwrap())
            );
            assert_eq!(rendered.data["unicode"], Value::String("ß東𝄞".to_owned()));
            assert_eq!(
                rendered.data["escapes"],
                Value::String("\u{0008}\u{000c}\n\r\t/".to_owned())
            );
        });
    }

    #[test]
    fn parses_whitespace_and_escaped_solidus_cases() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ntop_bool_ws=Template(' \\n true \\t ')\ntop_null_ws=Template(' \\r\\n null \\n')\nempty_string=t'\"\"'\nempty_object=Template('{ \\n\\t }')\nempty_array=Template('[ \\n\\t ]')\narray_empty_values=Template('[\"\", 0, false, null, {}, []]')\nempty_object_in_array=Template('[{}, {\"a\": []}]')\ntop_level_empty_object_ws=Template(' \\n { } \\t ')\nescaped_controls=t'\"\\\\b\\\\f\\\\n\\\\r\\\\t\"'\nescaped_solidus=t'\"\\\\/\"'\nescaped_backslash=t'\"\\\\\\\\\"'\nunicode_backslash_escape=t'\"\\\\u005C\"'\nreverse_solidus_u=t'\"\\\\u005C/\"'\nescaped_quote_backslash=t'\"\\\\\\\"\\\\\\\\\"'\nescaped_null_and_unit_separator=t'\"\\\\u0000\\\\u001f\"'\nnested_upper_unicode=t'\"\\\\u00DF\\\\u6771\"'\nunicode_line_sep=t'\"\\\\u2028\"'\nunicode_para_sep=t'\"\\\\u2029\"'\narray_with_line_sep=t'[\"\\\\u2028\", \"\\\\u2029\"]'\nunicode_escapes_array=Template('[\"\\\\u005C\", \"\\\\/\", \"\\\\u00DF\"]')\nunicode_mix_nested_obj=Template('{\"x\": {\"a\": \"\\\\u005C\", \"b\": \"\\\\u00DF\", \"c\": \"\\\\u2029\"}}')\nnested_unicode_object_array=Template('{\"a\": [{\"b\": \"\\\\u005C\", \"c\": \"\\\\u00DF\"}]}')\nupper_unicode_mix_array=Template('[\"\\\\u00DF\", \"\\\\u6771\", \"\\\\u2028\"]')\nescaped_slash_backslash_quote=t'\"\\\\/\\\\\\\\\\\\\\\"\"'\nescaped_reverse_solidus_solidus=t'\"\\\\\\\\/\"'\nnested_escaped_mix=Template('{\"x\":\"\\\\b\\\\u2028\\\\u2029\\\\/\"}')\nupper_exp=t'1E2'\nupper_exp_plus=t'1E+2'\nupper_exp_negative=t'-1E+2'\nupper_exp_zero_fraction=t'0E+0'\nupper_zero_negative_exp=t'-0E-0'\nnested_upper_exp=Template('{\"value\": 1E+2}')\nneg_exp_zero=t'-1e-0'\nupper_exp_negative_zero=t'1E-0'\nexp_with_fraction_zero=t'1.0e-0'\nnegative_zero_exp_upper=t'-0E0'\nnested_bool_null_mix=Template('{\"v\": [true, null, false, {\"x\": 1}]}')\nkeyword_array=Template('[true,false,null]')\nempty_name_nested_keywords=Template('{\"\": [null, true, false]}')\nnested_empty_mix=Template('{\"a\": [{}, [], \"\", 0, false, null]}')\nnested_empty_collections_mix=Template('{\"a\": {\"b\": []}, \"c\": [{}, []]}')\narray_nested_mixed_scalars=Template('[{\"a\": []}, {\"b\": {}}, \"\", 0, false, null]')\nnested_negative_exp_mix=Template('{\"x\":[-1E-2,0,\"\",{\"y\":[null]}]}')\nmixed_nested_keywords=Template('{\"a\": [true, false, null], \"b\": {\"c\": -1e-0}}')\nnested_number_combo=Template('{\"a\": [0, -0, -0.0, 1e0, -1E-0]}')\nnested_number_whitespace=Template('{\"a\": [ 0 , -0 , 1.5E-2 ] }')\nnested_empty_names=Template('{\"\": {\"\": []}}')\nnested_empty_name_array=Template('{\"\": [\"\", {\"\": 0}]}')\nnested_nulls=Template('{\"a\": null, \"b\": [null, {\"c\": null}]}')\nnested_top_ws=Template('\\r\\n {\"a\": [1, {\"b\": \"c\"}], \"\": \"\"} \\n')\ntop_ws_string=Template('\\n\\r\\t \"x\" \\n')\nzero_fraction_exp=t'0.0e+0'\nnested=Template('[\\n {\"a\": 1, \"b\": [true, false, null]}\\n]')\n"
                ),
                pyo3::ffi::c_str!("test_json_whitespace.py"),
                pyo3::ffi::c_str!("test_json_whitespace"),
            )
            .unwrap();

            let top_bool_ws = module.getattr("top_bool_ws").unwrap();
            let top_bool_ws = extract_template(py, &top_bool_ws, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&top_bool_ws).unwrap()).unwrap();
            assert_eq!(rendered.data, Value::Bool(true));

            let top_null_ws = module.getattr("top_null_ws").unwrap();
            let top_null_ws = extract_template(py, &top_null_ws, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&top_null_ws).unwrap()).unwrap();
            assert_eq!(rendered.data, Value::Null);

            let empty_string = module.getattr("empty_string").unwrap();
            let empty_string = extract_template(py, &empty_string, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&empty_string).unwrap()).unwrap();
            assert_eq!(rendered.data, Value::String(String::new()));

            let empty_object = module.getattr("empty_object").unwrap();
            let empty_object = extract_template(py, &empty_object, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&empty_object).unwrap()).unwrap();
            assert_eq!(rendered.data, Value::Object(Default::default()));

            let empty_array = module.getattr("empty_array").unwrap();
            let empty_array = extract_template(py, &empty_array, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&empty_array).unwrap()).unwrap();
            assert_eq!(rendered.data, Value::Array(Vec::new()));

            let escaped_controls = module.getattr("escaped_controls").unwrap();
            let escaped_controls =
                extract_template(py, &escaped_controls, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&escaped_controls).unwrap()).unwrap();
            assert_eq!(
                rendered.data,
                Value::String("\u{0008}\u{000c}\n\r\t".to_owned())
            );

            let escaped_solidus = module.getattr("escaped_solidus").unwrap();
            let escaped_solidus =
                extract_template(py, &escaped_solidus, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&escaped_solidus).unwrap()).unwrap();
            assert_eq!(rendered.data, Value::String("/".to_owned()));

            let escaped_backslash = module.getattr("escaped_backslash").unwrap();
            let escaped_backslash =
                extract_template(py, &escaped_backslash, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&escaped_backslash).unwrap()).unwrap();
            assert_eq!(rendered.data, Value::String("\\".to_owned()));

            let unicode_backslash_escape = module.getattr("unicode_backslash_escape").unwrap();
            let unicode_backslash_escape =
                extract_template(py, &unicode_backslash_escape, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&unicode_backslash_escape).unwrap()).unwrap();
            assert_eq!(rendered.data, Value::String("\\".to_owned()));

            let reverse_solidus_u = module.getattr("reverse_solidus_u").unwrap();
            let reverse_solidus_u =
                extract_template(py, &reverse_solidus_u, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&reverse_solidus_u).unwrap()).unwrap();
            assert_eq!(rendered.data, Value::String("\\/".to_owned()));

            let escaped_quote_backslash = module.getattr("escaped_quote_backslash").unwrap();
            let escaped_quote_backslash =
                extract_template(py, &escaped_quote_backslash, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&escaped_quote_backslash).unwrap()).unwrap();
            assert_eq!(rendered.data, Value::String("\"\\".to_owned()));

            let escaped_null_and_unit_separator =
                module.getattr("escaped_null_and_unit_separator").unwrap();
            let escaped_null_and_unit_separator =
                extract_template(py, &escaped_null_and_unit_separator, "json_t/json_t_str")
                    .unwrap();
            let rendered = render_document(
                py,
                &parse_template(&escaped_null_and_unit_separator).unwrap(),
            )
            .unwrap();
            assert_eq!(rendered.data, Value::String("\u{0000}\u{001f}".to_owned()));

            let nested_upper_unicode = module.getattr("nested_upper_unicode").unwrap();
            let nested_upper_unicode =
                extract_template(py, &nested_upper_unicode, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&nested_upper_unicode).unwrap()).unwrap();
            assert_eq!(rendered.data, Value::String("ß東".to_owned()));

            let unicode_line_sep = module.getattr("unicode_line_sep").unwrap();
            let unicode_line_sep =
                extract_template(py, &unicode_line_sep, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&unicode_line_sep).unwrap()).unwrap();
            assert_eq!(rendered.data, Value::String("\u{2028}".to_owned()));

            let unicode_para_sep = module.getattr("unicode_para_sep").unwrap();
            let unicode_para_sep =
                extract_template(py, &unicode_para_sep, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&unicode_para_sep).unwrap()).unwrap();
            assert_eq!(rendered.data, Value::String("\u{2029}".to_owned()));

            let array_with_line_sep = module.getattr("array_with_line_sep").unwrap();
            let array_with_line_sep =
                extract_template(py, &array_with_line_sep, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&array_with_line_sep).unwrap()).unwrap();
            assert_eq!(rendered.data.as_array().expect("array").len(), 2);
            assert_eq!(rendered.data[0], Value::String("\u{2028}".to_owned()));
            assert_eq!(rendered.data[1], Value::String("\u{2029}".to_owned()));

            let unicode_escapes_array = module.getattr("unicode_escapes_array").unwrap();
            let unicode_escapes_array =
                extract_template(py, &unicode_escapes_array, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&unicode_escapes_array).unwrap()).unwrap();
            let values = rendered.data.as_array().expect("array");
            assert_eq!(values[0], Value::String("\\".to_owned()));
            assert_eq!(values[1], Value::String("/".to_owned()));
            assert_eq!(values[2], Value::String("ß".to_owned()));

            let unicode_mix_nested_obj = module.getattr("unicode_mix_nested_obj").unwrap();
            let unicode_mix_nested_obj =
                extract_template(py, &unicode_mix_nested_obj, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&unicode_mix_nested_obj).unwrap()).unwrap();
            assert_eq!(rendered.data["x"]["a"], Value::String("\\".to_owned()));
            assert_eq!(rendered.data["x"]["b"], Value::String("ß".to_owned()));
            assert_eq!(
                rendered.data["x"]["c"],
                Value::String("\u{2029}".to_owned())
            );

            let nested_unicode_object_array =
                module.getattr("nested_unicode_object_array").unwrap();
            let nested_unicode_object_array =
                extract_template(py, &nested_unicode_object_array, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&nested_unicode_object_array).unwrap())
                    .unwrap();
            assert_eq!(rendered.data["a"][0]["b"], Value::String("\\".to_owned()));
            assert_eq!(rendered.data["a"][0]["c"], Value::String("ß".to_owned()));

            let upper_unicode_mix_array = module.getattr("upper_unicode_mix_array").unwrap();
            let upper_unicode_mix_array =
                extract_template(py, &upper_unicode_mix_array, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&upper_unicode_mix_array).unwrap()).unwrap();
            let values = rendered.data.as_array().expect("array");
            assert_eq!(values[0], Value::String("ß".to_owned()));
            assert_eq!(values[1], Value::String("東".to_owned()));
            assert_eq!(values[2], Value::String("\u{2028}".to_owned()));

            let escaped_slash_backslash_quote =
                module.getattr("escaped_slash_backslash_quote").unwrap();
            let escaped_slash_backslash_quote =
                extract_template(py, &escaped_slash_backslash_quote, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&escaped_slash_backslash_quote).unwrap())
                    .unwrap();
            assert_eq!(rendered.data, Value::String("/\\\"".to_owned()));

            let array_empty_values = module.getattr("array_empty_values").unwrap();
            let array_empty_values =
                extract_template(py, &array_empty_values, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&array_empty_values).unwrap()).unwrap();
            let values = rendered.data.as_array().expect("array");
            assert_eq!(values.len(), 6);
            assert_eq!(values[0], Value::String(String::new()));
            assert_eq!(values[1], Value::Number(Number::from(0)));
            assert_eq!(values[2], Value::Bool(false));
            assert_eq!(values[3], Value::Null);
            assert_eq!(values[4], Value::Object(Map::new()));
            assert_eq!(values[5], Value::Array(Vec::new()));

            let empty_object_in_array = module.getattr("empty_object_in_array").unwrap();
            let empty_object_in_array =
                extract_template(py, &empty_object_in_array, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&empty_object_in_array).unwrap()).unwrap();
            let values = rendered.data.as_array().expect("array");
            assert_eq!(values.len(), 2);
            assert_eq!(values[0], Value::Object(Map::new()));
            assert_eq!(values[1]["a"], Value::Array(Vec::new()));

            let top_level_empty_object_ws = module.getattr("top_level_empty_object_ws").unwrap();
            let top_level_empty_object_ws =
                extract_template(py, &top_level_empty_object_ws, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&top_level_empty_object_ws).unwrap()).unwrap();
            assert_eq!(rendered.data, Value::Object(Map::new()));

            let nested_escaped_mix = module.getattr("nested_escaped_mix").unwrap();
            let nested_escaped_mix =
                extract_template(py, &nested_escaped_mix, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&nested_escaped_mix).unwrap()).unwrap();
            assert_eq!(
                rendered.data["x"],
                Value::String("\u{0008}\u{2028}\u{2029}/".to_owned())
            );

            let escaped_reverse_solidus_solidus =
                module.getattr("escaped_reverse_solidus_solidus").unwrap();
            let escaped_reverse_solidus_solidus =
                extract_template(py, &escaped_reverse_solidus_solidus, "json_t/json_t_str")
                    .unwrap();
            let rendered = render_document(
                py,
                &parse_template(&escaped_reverse_solidus_solidus).unwrap(),
            )
            .unwrap();
            assert_eq!(rendered.data, Value::String("\\/".to_owned()));

            let upper_exp = module.getattr("upper_exp").unwrap();
            let upper_exp = extract_template(py, &upper_exp, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&upper_exp).unwrap()).unwrap();
            assert_eq!(
                rendered.data,
                Value::Number(Number::from_f64(100.0).unwrap())
            );

            let upper_exp_plus = module.getattr("upper_exp_plus").unwrap();
            let upper_exp_plus =
                extract_template(py, &upper_exp_plus, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&upper_exp_plus).unwrap()).unwrap();
            assert_eq!(
                rendered.data,
                Value::Number(Number::from_f64(100.0).unwrap())
            );

            let upper_exp_negative = module.getattr("upper_exp_negative").unwrap();
            let upper_exp_negative =
                extract_template(py, &upper_exp_negative, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&upper_exp_negative).unwrap()).unwrap();
            assert_eq!(
                rendered.data,
                Value::Number(Number::from_f64(-100.0).unwrap())
            );

            let upper_exp_zero_fraction = module.getattr("upper_exp_zero_fraction").unwrap();
            let upper_exp_zero_fraction =
                extract_template(py, &upper_exp_zero_fraction, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&upper_exp_zero_fraction).unwrap()).unwrap();
            assert_eq!(rendered.data, Value::Number(Number::from_f64(0.0).unwrap()));

            let upper_zero_negative_exp = module.getattr("upper_zero_negative_exp").unwrap();
            let upper_zero_negative_exp =
                extract_template(py, &upper_zero_negative_exp, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&upper_zero_negative_exp).unwrap()).unwrap();
            assert_eq!(
                rendered.data,
                Value::Number(Number::from_f64(-0.0).unwrap())
            );

            let nested_upper_exp = module.getattr("nested_upper_exp").unwrap();
            let nested_upper_exp =
                extract_template(py, &nested_upper_exp, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&nested_upper_exp).unwrap()).unwrap();
            assert_eq!(
                rendered.data["value"],
                Value::Number(Number::from_f64(100.0).unwrap())
            );

            let neg_exp_zero = module.getattr("neg_exp_zero").unwrap();
            let neg_exp_zero = extract_template(py, &neg_exp_zero, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&neg_exp_zero).unwrap()).unwrap();
            assert_eq!(
                rendered.data,
                Value::Number(Number::from_f64(-1.0).unwrap())
            );

            let upper_exp_negative_zero = module.getattr("upper_exp_negative_zero").unwrap();
            let upper_exp_negative_zero =
                extract_template(py, &upper_exp_negative_zero, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&upper_exp_negative_zero).unwrap()).unwrap();
            assert_eq!(rendered.data, Value::Number(Number::from_f64(1.0).unwrap()));

            let exp_with_fraction_zero = module.getattr("exp_with_fraction_zero").unwrap();
            let exp_with_fraction_zero =
                extract_template(py, &exp_with_fraction_zero, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&exp_with_fraction_zero).unwrap()).unwrap();
            assert_eq!(rendered.data, Value::Number(Number::from_f64(1.0).unwrap()));

            let negative_zero_exp_upper = module.getattr("negative_zero_exp_upper").unwrap();
            let negative_zero_exp_upper =
                extract_template(py, &negative_zero_exp_upper, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&negative_zero_exp_upper).unwrap()).unwrap();
            assert_eq!(
                rendered.data,
                Value::Number(Number::from_f64(-0.0).unwrap())
            );

            let nested_negative_exp_mix = module.getattr("nested_negative_exp_mix").unwrap();
            let nested_negative_exp_mix =
                extract_template(py, &nested_negative_exp_mix, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&nested_negative_exp_mix).unwrap()).unwrap();
            assert_eq!(
                rendered.data["x"][0],
                Value::Number(Number::from_f64(-0.01).unwrap())
            );
            assert_eq!(rendered.data["x"][1], Value::Number(Number::from(0)));
            assert_eq!(rendered.data["x"][2], Value::String(String::new()));
            assert_eq!(rendered.data["x"][3]["y"][0], Value::Null);

            let mixed_nested_keywords = module.getattr("mixed_nested_keywords").unwrap();
            let mixed_nested_keywords =
                extract_template(py, &mixed_nested_keywords, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&mixed_nested_keywords).unwrap()).unwrap();
            assert_eq!(rendered.data["a"][0], Value::Bool(true));
            assert_eq!(rendered.data["a"][1], Value::Bool(false));
            assert_eq!(rendered.data["a"][2], Value::Null);
            assert_eq!(
                rendered.data["b"]["c"],
                Value::Number(Number::from_f64(-1.0).unwrap())
            );

            let nested_empty_names = module.getattr("nested_empty_names").unwrap();
            let nested_empty_names =
                extract_template(py, &nested_empty_names, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&nested_empty_names).unwrap()).unwrap();
            assert_eq!(rendered.data[""][""], Value::Array(Vec::new()));

            let nested_empty_name_array = module.getattr("nested_empty_name_array").unwrap();
            let nested_empty_name_array =
                extract_template(py, &nested_empty_name_array, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&nested_empty_name_array).unwrap()).unwrap();
            assert_eq!(rendered.data[""][0], Value::String(String::new()));
            assert_eq!(rendered.data[""][1][""], Value::Number(Number::from(0)));

            let nested_nulls = module.getattr("nested_nulls").unwrap();
            let nested_nulls = extract_template(py, &nested_nulls, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&nested_nulls).unwrap()).unwrap();
            assert_eq!(rendered.data["a"], Value::Null);
            assert_eq!(rendered.data["b"][0], Value::Null);
            assert_eq!(rendered.data["b"][1]["c"], Value::Null);

            let nested_top_ws = module.getattr("nested_top_ws").unwrap();
            let nested_top_ws = extract_template(py, &nested_top_ws, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&nested_top_ws).unwrap()).unwrap();
            assert_eq!(rendered.data["a"][1]["b"], Value::String("c".to_owned()));
            assert_eq!(rendered.data[""], Value::String(String::new()));

            let top_ws_string = module.getattr("top_ws_string").unwrap();
            let top_ws_string = extract_template(py, &top_ws_string, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&top_ws_string).unwrap()).unwrap();
            assert_eq!(rendered.data, Value::String("x".to_owned()));

            let nested_number_whitespace = module.getattr("nested_number_whitespace").unwrap();
            let nested_number_whitespace =
                extract_template(py, &nested_number_whitespace, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&nested_number_whitespace).unwrap()).unwrap();
            let values = rendered.data["a"].as_array().expect("array");
            assert_eq!(values.len(), 3);
            assert_eq!(values[0], Value::Number(Number::from(0)));
            assert_eq!(values[1], Value::Number(Number::from_f64(-0.0).unwrap()));
            assert_eq!(values[2], Value::Number(Number::from_f64(0.015).unwrap()));

            let nested_number_combo = module.getattr("nested_number_combo").unwrap();
            let nested_number_combo =
                extract_template(py, &nested_number_combo, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&nested_number_combo).unwrap()).unwrap();
            let values = rendered.data["a"].as_array().expect("array");
            assert_eq!(values[0], Value::Number(Number::from(0)));
            assert_eq!(values[1], Value::Number(Number::from_f64(-0.0).unwrap()));
            assert_eq!(values[2], Value::Number(Number::from_f64(-0.0).unwrap()));
            assert_eq!(values[3], Value::Number(Number::from_f64(1.0).unwrap()));
            assert_eq!(values[4], Value::Number(Number::from_f64(-1.0).unwrap()));

            let nested_bool_null_mix = module.getattr("nested_bool_null_mix").unwrap();
            let nested_bool_null_mix =
                extract_template(py, &nested_bool_null_mix, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&nested_bool_null_mix).unwrap()).unwrap();
            assert_eq!(rendered.data["v"][0], Value::Bool(true));
            assert_eq!(rendered.data["v"][1], Value::Null);
            assert_eq!(rendered.data["v"][2], Value::Bool(false));
            assert_eq!(rendered.data["v"][3]["x"], Value::Number(Number::from(1)));

            let keyword_array = module.getattr("keyword_array").unwrap();
            let keyword_array = extract_template(py, &keyword_array, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&keyword_array).unwrap()).unwrap();
            let values = rendered.data.as_array().expect("array");
            assert_eq!(values[0], Value::Bool(true));
            assert_eq!(values[1], Value::Bool(false));
            assert_eq!(values[2], Value::Null);

            let empty_name_nested_keywords = module.getattr("empty_name_nested_keywords").unwrap();
            let empty_name_nested_keywords =
                extract_template(py, &empty_name_nested_keywords, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&empty_name_nested_keywords).unwrap()).unwrap();
            let values = rendered.data[""].as_array().expect("array");
            assert_eq!(values[0], Value::Null);
            assert_eq!(values[1], Value::Bool(true));
            assert_eq!(values[2], Value::Bool(false));

            let nested_empty_mix = module.getattr("nested_empty_mix").unwrap();
            let nested_empty_mix =
                extract_template(py, &nested_empty_mix, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&nested_empty_mix).unwrap()).unwrap();
            let values = rendered.data["a"].as_array().expect("array");
            assert_eq!(values[0], Value::Object(Map::new()));
            assert_eq!(values[1], Value::Array(Vec::new()));
            assert_eq!(values[2], Value::String(String::new()));
            assert_eq!(values[3], Value::Number(Number::from(0)));
            assert_eq!(values[4], Value::Bool(false));
            assert_eq!(values[5], Value::Null);

            let nested_empty_collections_mix =
                module.getattr("nested_empty_collections_mix").unwrap();
            let nested_empty_collections_mix =
                extract_template(py, &nested_empty_collections_mix, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&nested_empty_collections_mix).unwrap())
                    .unwrap();
            assert_eq!(rendered.data["a"]["b"], Value::Array(Vec::new()));
            assert_eq!(rendered.data["c"][0], Value::Object(Map::new()));
            assert_eq!(rendered.data["c"][1], Value::Array(Vec::new()));

            let array_nested_mixed_scalars = module.getattr("array_nested_mixed_scalars").unwrap();
            let array_nested_mixed_scalars =
                extract_template(py, &array_nested_mixed_scalars, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&array_nested_mixed_scalars).unwrap()).unwrap();
            let values = rendered.data.as_array().expect("array");
            assert_eq!(values[0]["a"], Value::Array(Vec::new()));
            assert_eq!(values[1]["b"], Value::Object(Map::new()));
            assert_eq!(values[2], Value::String(String::new()));
            assert_eq!(values[3], Value::Number(Number::from(0)));
            assert_eq!(values[4], Value::Bool(false));
            assert_eq!(values[5], Value::Null);

            let zero_fraction_exp = module.getattr("zero_fraction_exp").unwrap();
            let zero_fraction_exp =
                extract_template(py, &zero_fraction_exp, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&zero_fraction_exp).unwrap()).unwrap();
            assert_eq!(rendered.data, Value::Number(Number::from_f64(0.0).unwrap()));

            let nested = module.getattr("nested").unwrap();
            let nested = extract_template(py, &nested, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&nested).unwrap()).unwrap();
            assert_eq!(rendered.data.as_array().expect("array").len(), 1);
            assert_eq!(rendered.data[0]["a"], Value::Number(Number::from(1)));
            assert_eq!(rendered.data[0]["b"][0], Value::Bool(true));
            assert_eq!(rendered.data[0]["b"][1], Value::Bool(false));
            assert_eq!(rendered.data[0]["b"][2], Value::Null);
        });
    }

    #[test]
    fn rejects_invalid_unicode_and_control_sequences() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nincomplete=Template('\"\\\\u12\"')\ncontrol=Template('\"a\\nb\"')\nlow=Template('\"\\\\uDD1E\"')\nexp=Template('1e+')\nexp2=Template('1e')\ndot=Template('1.')\nminus=Template('-')\nobject_exp=Template('{\"a\": 1e+}')\nobject_dot=Template('{\"a\": 1.}')\ntrailing_true=Template('true false')\ntrailing_string=Template('\"a\" \"b\"')\nextra_close=Template('[1]]')\nobject_extra_close=Template('{\"a\":1}}')\ndouble_trailing_close=Template('{\"a\": [1]}]')\nmissing_array_comma=Template('[1 2]')\nmissing_object_comma=Template('{\"a\":1 \"b\":2}')\nnested_missing_object_array_comma=Template('{\"a\": [1 2]}')\nextra_array_comma=Template('[1,,2]')\nleading_comma_array=Template('[,1]')\nmissing_value=Template('{\"a\":,1}')\nmissing_key=Template('{,}')\nmissing_colon=Template('{\"a\" 1}')\ntrailing_object_comma=Template('{\"a\":1,}')\ntrailing_array_comma=Template('[1,2,]')\nnested_trailing_object_comma=Template('[{\"a\":1,}]')\nplus_number=Template('+1')\nleading_decimal=Template('.1')\nleading_zero=Template('00')\nnegative_leading_zero=Template('-01')\nleading_zero_exp=Template('01e0')\nnegative_leading_zero_exp=Template('-01e0')\nmissing_exp_digits_minus=Template('1e-')\nobject_missing_exp_digits_minus=Template('{\"x\": 1e-}')\nobject_negative_leading_zero=Template('{\"a\": -01}')\narray_double_value_no_comma=Template('[true false]')\nnested_missing_comma_bool=Template('{\"a\": [true false]}')\narray_true_number_no_comma=Template('[true 1]')\narray_false_object_no_comma=Template('[false {\"a\":1}]')\nnested_missing_comma_obj_after_null=Template('[null {\"a\":1}]')\nobject_double_value_no_comma=Template('{\"a\": true false}')\nobject_keyword_number_no_comma=Template('{\"a\": true 1}')\ntruncated_true=Template('tru')\ntruncated_false=Template('fals')\ntruncated_null=Template('nul')\nobject_truncated_true=Template('{\"a\": tru}')\nobject_truncated_false=Template('{\"a\": fals}')\nobject_truncated_null=Template('{\"a\": nul}')\narray_truncated_true=Template('[tru]')\narray_truncated_false=Template('[fals]')\nnested_array_truncated_true=Template('{\"a\": [tru]}')\nnested_array_truncated_false=Template('{\"a\": [fals]}')\narray_truncated_null=Template('[nul]')\narray_bad_null_case=Template('[nulL]')\nnested_array_bad_null_case=Template('{\"a\": [nulL]}')\nnested_object_truncated_null=Template('[{\"a\": nul}]')\nkeyword_prefix=Template('[truee]')\nbad_true_case=Template('truE')\narray_bad_true_case=Template('[truE]')\nbad_false_case=Template('falsE')\nbad_null_case=Template('nulL')\nobject_bad_true_case=Template('{\"a\": truE}')\nobject_bad_false_case=Template('{\"a\": falsE}')\nobject_bad_null_case=Template('{\"a\": nulL}')\narray_bad_false_case=Template('[falsE]')\narray_missing_comma_after_string=Template('[\"a\" true]')\nobject_missing_comma_after_null=Template('{\"a\": null \"b\": 1}')\ndouble_decimal_point=Template('1.2.3')\nobject_bad_leading_zero=Template('{\"a\": 00}')\nnested_extra_close=Template('[{\"a\":1}]]')\n"
                ),
                pyo3::ffi::c_str!("test_json_invalid_unicode.py"),
                pyo3::ffi::c_str!("test_json_invalid_unicode"),
            )
            .unwrap();

            for (name, expected) in [
                ("incomplete", "Unexpected end of JSON escape sequence"),
                ("control", "Control characters are not allowed"),
                ("low", "Invalid JSON unicode escape"),
                ("exp", "Invalid JSON number literal"),
                ("exp2", "Invalid JSON number literal"),
                ("dot", "Invalid JSON number literal"),
                ("minus", "Invalid JSON number literal"),
                ("object_exp", "Invalid JSON number literal"),
                ("object_dot", "Invalid JSON number literal"),
                ("trailing_true", "Expected a JSON value"),
                (
                    "trailing_string",
                    "Unexpected trailing content in JSON template",
                ),
                (
                    "extra_close",
                    "Unexpected trailing content in JSON template",
                ),
                (
                    "object_extra_close",
                    "Unexpected trailing content in JSON template",
                ),
                (
                    "double_trailing_close",
                    "Unexpected trailing content in JSON template",
                ),
                ("missing_array_comma", "Invalid JSON number literal"),
                ("missing_object_comma", "Invalid JSON number literal"),
                (
                    "nested_missing_object_array_comma",
                    "Invalid JSON number literal",
                ),
                ("extra_array_comma", "Expected a JSON value"),
                ("leading_comma_array", "Expected a JSON value"),
                ("missing_value", "Expected a JSON value"),
                (
                    "missing_key",
                    "JSON object keys must be quoted strings or interpolations",
                ),
                ("missing_colon", "Expected ':' in JSON template"),
                (
                    "trailing_object_comma",
                    "JSON object keys must be quoted strings or interpolations",
                ),
                ("trailing_array_comma", "Expected a JSON value"),
                (
                    "nested_trailing_object_comma",
                    "quoted strings or interpolations",
                ),
                ("plus_number", "Expected a JSON value"),
                ("leading_decimal", "Expected a JSON value"),
                ("leading_zero", "Invalid JSON number literal"),
                ("negative_leading_zero", "Invalid JSON number literal"),
                ("leading_zero_exp", "Invalid JSON number literal"),
                ("negative_leading_zero_exp", "Invalid JSON number literal"),
                ("missing_exp_digits_minus", "Invalid JSON number literal"),
                (
                    "object_missing_exp_digits_minus",
                    "Invalid JSON number literal",
                ),
                (
                    "object_negative_leading_zero",
                    "Invalid JSON number literal",
                ),
                ("array_double_value_no_comma", "Expected a JSON value"),
                ("nested_missing_comma_bool", "Expected a JSON value"),
                ("array_true_number_no_comma", "Expected a JSON value"),
                (
                    "array_false_object_no_comma",
                    "Invalid promoted JSON fragment content",
                ),
                (
                    "nested_missing_comma_obj_after_null",
                    "Invalid promoted JSON fragment content",
                ),
                ("object_double_value_no_comma", "Expected a JSON value"),
                ("object_keyword_number_no_comma", "Expected a JSON value"),
                ("truncated_true", "Expected a JSON value"),
                ("truncated_false", "Expected a JSON value"),
                ("truncated_null", "Expected a JSON value"),
                ("object_truncated_true", "Expected a JSON value"),
                ("object_truncated_false", "Expected a JSON value"),
                ("object_truncated_null", "Expected a JSON value"),
                ("array_truncated_true", "Expected a JSON value"),
                ("array_truncated_false", "Expected a JSON value"),
                ("nested_array_truncated_true", "Expected a JSON value"),
                ("nested_array_truncated_false", "Expected a JSON value"),
                ("array_truncated_null", "Expected a JSON value"),
                ("nested_object_truncated_null", "Expected a JSON value"),
                ("keyword_prefix", "Expected a JSON value"),
                ("bad_true_case", "Expected a JSON value"),
                ("array_bad_true_case", "Expected a JSON value"),
                ("bad_false_case", "Expected a JSON value"),
                ("bad_null_case", "Expected a JSON value"),
                ("object_bad_true_case", "Expected a JSON value"),
                ("object_bad_false_case", "Expected a JSON value"),
                ("object_bad_null_case", "Expected a JSON value"),
                ("array_bad_false_case", "Expected a JSON value"),
                ("array_bad_null_case", "Expected a JSON value"),
                ("nested_array_bad_null_case", "Expected a JSON value"),
                (
                    "array_missing_comma_after_string",
                    "Expected ',' in JSON template",
                ),
                (
                    "object_missing_comma_after_null",
                    "Invalid promoted JSON fragment content",
                ),
                ("double_decimal_point", "Invalid JSON number literal"),
                ("object_bad_leading_zero", "Invalid JSON number literal"),
                (
                    "nested_extra_close",
                    "Unexpected trailing content in JSON template",
                ),
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "json_t/json_t_str").unwrap();
                let err = match parse_template(&template) {
                    Ok(_) => panic!("expected JSON parse failure for {name}"),
                    Err(err) => err,
                };
                assert_eq!(err.kind, ErrorKind::Parse);
                assert!(err.message.contains(expected), "{name}: {}", err.message);
            }
        });
    }

    #[test]
    fn rejects_structural_invalid_message_families() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nmissing_comma=Template('[\"a\" true]')\ntrailing_comma=Template('{\"a\":1,}')\ninvalid_fragment=Template('[null {\"a\":1}]')\nunexpected_trailing=Template('{\"a\":1}}')\n"
                ),
                pyo3::ffi::c_str!("test_json_structural_invalids.py"),
                pyo3::ffi::c_str!("test_json_structural_invalids"),
            )
            .unwrap();

            for (name, expected) in [
                ("missing_comma", "Expected ',' in JSON template"),
                (
                    "trailing_comma",
                    "JSON object keys must be quoted strings or interpolations",
                ),
                ("invalid_fragment", "Invalid promoted JSON fragment content"),
                (
                    "unexpected_trailing",
                    "Unexpected trailing content in JSON template",
                ),
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "json_t/json_t_str").unwrap();
                let err = parse_template(&template).expect_err("expected JSON parse failure");
                assert_eq!(err.kind, ErrorKind::Parse);
                assert!(err.message.contains(expected), "{name}: {}", err.message);
            }
        });
    }

    #[test]
    fn rejects_keyword_truncation_and_collection_separator_errors() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nbad_true=Template('[tru]')\nbad_false=Template('{\"a\": falsE}')\nbad_null=Template('[nulL]')\nmissing_comma_array=Template('[null {\"a\": 1}]')\ntrailing_array_comma=Template('[1,2,]')\ntrailing_object_comma=Template('{\"a\":1,}')\n"
                ),
                pyo3::ffi::c_str!("test_json_invalid_keywords.py"),
                pyo3::ffi::c_str!("test_json_invalid_keywords"),
            )
            .unwrap();

            for (name, expected) in [
                ("bad_true", "Expected a JSON value"),
                ("bad_false", "Expected a JSON value"),
                ("bad_null", "Expected a JSON value"),
                (
                    "missing_comma_array",
                    "Invalid promoted JSON fragment content",
                ),
                ("trailing_array_comma", "Expected a JSON value"),
                ("trailing_object_comma", "quoted strings or interpolations"),
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "json_t/json_t_str").unwrap();
                let err = parse_template(&template).expect_err("expected JSON parse failure");
                assert_eq!(err.kind, ErrorKind::Parse);
                assert!(err.message.contains(expected), "{name}: {}", err.message);
            }
        });
    }

    #[test]
    fn rejects_additional_number_and_trailing_content_families() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nleading_zero_exp=Template('01e0')\nleading_zero_exp_negative=Template('-01e0')\nmissing_exp_digits=Template('1e-')\nembedded_missing_exp_digits=Template('{\"x\": 1e-}')\ndouble_sign_number=Template('-+1')\nleading_plus_minus=Template('+-1')\nbad_exp_plus_minus=Template('1e+-1')\nbad_exp_minus_plus=Template('1e-+1')\nextra_decimal=Template('1.2.3')\narray_space_number=Template('[1 2]')\nobject_space_number=Template('{\"a\":1 \"b\":2}')\ntruee=Template('[truee]')\ntrue_then_number=Template('[true 1]')\nobject_true_then_number=Template('{\"a\": true 1}')\nfalse_fragment=Template('[false {\"a\":1}]')\narray_trailing_object=Template('{\"a\": [1]}]')\nobject_trailing_array=Template('[{\"a\":1}]]')\ndeep_object_trailing=Template('{\"a\": {\"b\": 1}}}')\n"
                ),
                pyo3::ffi::c_str!("test_json_additional_parse_errors.py"),
                pyo3::ffi::c_str!("test_json_additional_parse_errors"),
            )
            .unwrap();

            for (name, expected) in [
                ("leading_zero_exp", "Invalid JSON number literal"),
                ("leading_zero_exp_negative", "Invalid JSON number literal"),
                ("missing_exp_digits", "Invalid JSON number literal"),
                ("embedded_missing_exp_digits", "Invalid JSON number literal"),
                ("double_sign_number", "Invalid JSON number literal"),
                ("leading_plus_minus", "Expected a JSON value"),
                ("bad_exp_plus_minus", "Invalid JSON number literal"),
                ("bad_exp_minus_plus", "Invalid JSON number literal"),
                ("extra_decimal", "Invalid JSON number literal"),
                ("array_space_number", "Invalid JSON number literal"),
                ("object_space_number", "Invalid JSON number literal"),
                ("truee", "Expected a JSON value"),
                ("true_then_number", "Expected a JSON value"),
                ("object_true_then_number", "Expected a JSON value"),
                ("false_fragment", "Invalid promoted JSON fragment content"),
                (
                    "array_trailing_object",
                    "Unexpected trailing content in JSON template",
                ),
                (
                    "object_trailing_array",
                    "Unexpected trailing content in JSON template",
                ),
                (
                    "deep_object_trailing",
                    "Unexpected trailing content in JSON template",
                ),
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "json_t/json_t_str").unwrap();
                let err = parse_template(&template).expect_err("expected JSON parse failure");
                assert_eq!(err.kind, ErrorKind::Parse);
                assert!(err.message.contains(expected), "{name}: {}", err.message);
            }
        });
    }

    #[test]
    fn renders_keyword_and_empty_name_collection_shapes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nkeyword_array=Template('[true,false,null]')\nempty_name_nested_keywords=Template('{\"\": [null, true, false]}')\nnested_empty_mix=Template('{\"a\": [{}, [], \"\", 0, false, null]}')\narray_nested_mixed_scalars=Template('[{\"a\": []}, {\"b\": {}}, \"\", 0, false, null]')\n"
                ),
                pyo3::ffi::c_str!("test_json_keyword_empty_shapes.py"),
                pyo3::ffi::c_str!("test_json_keyword_empty_shapes"),
            )
            .unwrap();

            let keyword_array = module.getattr("keyword_array").unwrap();
            let keyword_array = extract_template(py, &keyword_array, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&keyword_array).unwrap()).unwrap();
            assert_eq!(rendered.text, "[true, false, null]");
            assert_eq!(rendered.data, json!([true, false, null]));

            let empty_name_nested_keywords = module.getattr("empty_name_nested_keywords").unwrap();
            let empty_name_nested_keywords =
                extract_template(py, &empty_name_nested_keywords, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&empty_name_nested_keywords).unwrap()).unwrap();
            assert_eq!(rendered.text, "{\"\": [null, true, false]}");
            assert_eq!(rendered.data, json!({"": [null, true, false]}));

            let nested_empty_mix = module.getattr("nested_empty_mix").unwrap();
            let nested_empty_mix =
                extract_template(py, &nested_empty_mix, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&nested_empty_mix).unwrap()).unwrap();
            assert_eq!(rendered.data, json!({"a": [{}, [], "", 0, false, null]}));

            let array_nested_mixed_scalars = module.getattr("array_nested_mixed_scalars").unwrap();
            let array_nested_mixed_scalars =
                extract_template(py, &array_nested_mixed_scalars, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&array_nested_mixed_scalars).unwrap()).unwrap();
            assert_eq!(
                rendered.data,
                json!([{"a": []}, {"b": {}}, "", 0, false, null])
            );
        });
    }

    #[test]
    fn renders_top_level_whitespace_and_nested_number_shapes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ntop_ws_string=Template('\\n\\r\\t \"x\" \\n')\nnested_top_ws=Template('\\r\\n {\"a\": [1, {\"b\": \"c\"}], \"\": \"\"} \\n')\nnested_number_whitespace=Template('{\"a\": [ 0 , -0 , 1.5E-2 ] }')\n"
                ),
                pyo3::ffi::c_str!("test_json_whitespace_shapes.py"),
                pyo3::ffi::c_str!("test_json_whitespace_shapes"),
            )
            .unwrap();

            let top_ws_string = module.getattr("top_ws_string").unwrap();
            let top_ws_string = extract_template(py, &top_ws_string, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&top_ws_string).unwrap()).unwrap();
            assert_eq!(rendered.text, "\"x\"");
            assert_eq!(rendered.data, json!("x"));

            let nested_top_ws = module.getattr("nested_top_ws").unwrap();
            let nested_top_ws = extract_template(py, &nested_top_ws, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&nested_top_ws).unwrap()).unwrap();
            assert_eq!(rendered.text, "{\"a\": [1, {\"b\": \"c\"}], \"\": \"\"}");
            assert_eq!(rendered.data, json!({"a": [1, {"b": "c"}], "": ""}));

            let nested_number_whitespace = module.getattr("nested_number_whitespace").unwrap();
            let nested_number_whitespace =
                extract_template(py, &nested_number_whitespace, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&nested_number_whitespace).unwrap()).unwrap();
            assert_eq!(rendered.text, "{\"a\": [0, -0, 1.5E-2]}");
            assert_eq!(
                rendered.data,
                serde_json::from_str::<Value>("{\"a\": [0, -0, 1.5E-2]}").unwrap()
            );
        });
    }

    #[test]
    fn renders_end_to_end_supported_positions_text_and_data() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "key='user'\nleft='prefix'\nright='suffix'\npayload={'enabled': True, 'count': 2}\ntemplate=t'''\n{{\n  {key}: {payload},\n  \"prefix-{left}\": \"item-{right}\",\n  \"label\": {left}-{right}\n}}\n'''\n"
                ),
                pyo3::ffi::c_str!("test_json_end_to_end_positions.py"),
                pyo3::ffi::c_str!("test_json_end_to_end_positions"),
            )
            .unwrap();

            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
            assert_eq!(
                rendered.text,
                "{\"user\": {\"enabled\": true, \"count\": 2}, \"prefix-prefix\": \"item-suffix\", \"label\": \"prefix-suffix\"}"
            );
            assert_eq!(
                rendered.data,
                json!({
                    "user": {"enabled": true, "count": 2},
                    "prefix-prefix": "item-suffix",
                    "label": "prefix-suffix",
                })
            );
        });
    }

    #[test]
    fn renders_rfc_8259_image_example_text_and_data() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "template=t'''{{\n  \"Image\": {{\n    \"Width\": 800,\n    \"Height\": 600,\n    \"Title\": \"View from 15th Floor\",\n    \"Thumbnail\": {{\n      \"Url\": \"http://www.example.com/image/481989943\",\n      \"Height\": 125,\n      \"Width\": 100\n    }},\n    \"Animated\": false,\n    \"IDs\": [116, 943, 234, 38793]\n  }}\n}}'''\n"
                ),
                pyo3::ffi::c_str!("test_json_rfc_8259_image_example.py"),
                pyo3::ffi::c_str!("test_json_rfc_8259_image_example"),
            )
            .unwrap();

            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
            assert_eq!(
                rendered.text,
                "{\"Image\": {\"Width\": 800, \"Height\": 600, \"Title\": \"View from 15th Floor\", \"Thumbnail\": {\"Url\": \"http://www.example.com/image/481989943\", \"Height\": 125, \"Width\": 100}, \"Animated\": false, \"IDs\": [116, 943, 234, 38793]}}"
            );
            assert_eq!(
                rendered.data,
                json!({
                    "Image": {
                        "Width": 800,
                        "Height": 600,
                        "Title": "View from 15th Floor",
                        "Thumbnail": {
                            "Url": "http://www.example.com/image/481989943",
                            "Height": 125,
                            "Width": 100,
                        },
                        "Animated": false,
                        "IDs": [116, 943, 234, 38793],
                    }
                })
            );
        });
    }

    #[test]
    fn renders_rfc_8259_value_examples_text_and_data() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "array=t'''[\n  {{\n     \"precision\": \"zip\",\n     \"Latitude\":  37.7668,\n     \"Longitude\": -122.3959,\n     \"Address\":   \"\",\n     \"City\":      \"SAN FRANCISCO\",\n     \"State\":     \"CA\",\n     \"Zip\":       \"94107\",\n     \"Country\":   \"US\"\n  }},\n  {{\n     \"precision\": \"zip\",\n     \"Latitude\":  37.371991,\n     \"Longitude\": -122.026020,\n     \"Address\":   \"\",\n     \"City\":      \"SUNNYVALE\",\n     \"State\":     \"CA\",\n     \"Zip\":       \"94085\",\n     \"Country\":   \"US\"\n  }}\n]'''\nstring=t'\"Hello world!\"'\nnumber=t'42'\nboolean=t'true'\n"
                ),
                pyo3::ffi::c_str!("test_json_rfc_8259_value_examples.py"),
                pyo3::ffi::c_str!("test_json_rfc_8259_value_examples"),
            )
            .unwrap();

            let array = module.getattr("array").unwrap();
            let array = extract_template(py, &array, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&array).unwrap()).unwrap();
            assert_eq!(
                rendered.text,
                "[{\"precision\": \"zip\", \"Latitude\": 37.7668, \"Longitude\": -122.3959, \"Address\": \"\", \"City\": \"SAN FRANCISCO\", \"State\": \"CA\", \"Zip\": \"94107\", \"Country\": \"US\"}, {\"precision\": \"zip\", \"Latitude\": 37.371991, \"Longitude\": -122.026020, \"Address\": \"\", \"City\": \"SUNNYVALE\", \"State\": \"CA\", \"Zip\": \"94085\", \"Country\": \"US\"}]"
            );
            assert_eq!(
                rendered.data,
                json!([
                    {
                        "precision": "zip",
                        "Latitude": 37.7668,
                        "Longitude": -122.3959,
                        "Address": "",
                        "City": "SAN FRANCISCO",
                        "State": "CA",
                        "Zip": "94107",
                        "Country": "US",
                    },
                    {
                        "precision": "zip",
                        "Latitude": 37.371991,
                        "Longitude": -122.026020,
                        "Address": "",
                        "City": "SUNNYVALE",
                        "State": "CA",
                        "Zip": "94085",
                        "Country": "US",
                    }
                ])
            );

            for (name, expected_text, expected_value) in [
                ("string", "\"Hello world!\"", json!("Hello world!")),
                ("number", "42", json!(42)),
                ("boolean", "true", json!(true)),
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "json_t/json_t_str").unwrap();
                let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
                assert_eq!(rendered.text, expected_text, "{name}");
                assert_eq!(rendered.data, expected_value, "{name}");
            }
        });
    }

    #[test]
    fn renders_unicode_and_escape_mix_shapes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "unicode_array=t'[\"\\\\u2028\", \"\\\\u2029\", \"\\\\u00DF\"]'\nescape_object=t'{{\"x\":\"\\\\b\\\\u2028\\\\u2029\\\\/\"}}'\n"
                ),
                pyo3::ffi::c_str!("test_json_unicode_escape_mix.py"),
                pyo3::ffi::c_str!("test_json_unicode_escape_mix"),
            )
            .unwrap();

            let unicode_array = module.getattr("unicode_array").unwrap();
            let unicode_array = extract_template(py, &unicode_array, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&unicode_array).unwrap()).unwrap();
            assert_eq!(rendered.text, "[\"\u{2028}\", \"\u{2029}\", \"ß\"]");
            assert_eq!(rendered.data, json!(["\u{2028}", "\u{2029}", "ß"]));

            let escape_object = module.getattr("escape_object").unwrap();
            let escape_object = extract_template(py, &escape_object, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&escape_object).unwrap()).unwrap();
            assert_eq!(rendered.text, "{\"x\": \"\\b\u{2028}\u{2029}/\"}");
            assert_eq!(rendered.data, json!({"x": "\u{0008}\u{2028}\u{2029}/"}));
        });
    }

    #[test]
    fn renders_control_escapes_and_reverse_solidus_variants() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "escaped_controls=t'\"\\\\b\\\\f\\\\n\\\\r\\\\t\"'\nreverse_solidus_u=t'\"\\\\u005C/\"'\nescaped_quote_backslash=t'\"\\\\\\\"\\\\\\\\\"'\n"
                ),
                pyo3::ffi::c_str!("test_json_escape_variants.py"),
                pyo3::ffi::c_str!("test_json_escape_variants"),
            )
            .unwrap();

            let escaped_controls = module.getattr("escaped_controls").unwrap();
            let escaped_controls =
                extract_template(py, &escaped_controls, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&escaped_controls).unwrap()).unwrap();
            assert_eq!(rendered.text, "\"\\b\\f\\n\\r\\t\"");
            assert_eq!(rendered.data, json!("\u{0008}\u{000c}\n\r\t"));

            let reverse_solidus_u = module.getattr("reverse_solidus_u").unwrap();
            let reverse_solidus_u =
                extract_template(py, &reverse_solidus_u, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&reverse_solidus_u).unwrap()).unwrap();
            assert_eq!(rendered.text, "\"\\\\/\"");
            assert_eq!(rendered.data, json!("\\/"));

            let escaped_quote_backslash = module.getattr("escaped_quote_backslash").unwrap();
            let escaped_quote_backslash =
                extract_template(py, &escaped_quote_backslash, "json_t/json_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&escaped_quote_backslash).unwrap()).unwrap();
            assert_eq!(rendered.text, "\"\\\"\\\\\"");
            assert_eq!(rendered.data, json!("\"\\"));
        });
    }

    #[test]
    fn renders_promoted_rows_and_fragment_text_shapes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "left='prefix'\nright='suffix'\nrows=[{'name': 'one'}, {'name': 'two'}]\nfragment=t'{{\"label\": {left}-{right}}}'\npromoted=t'{rows}'\n"
                ),
                pyo3::ffi::c_str!("test_json_promoted_rows.py"),
                pyo3::ffi::c_str!("test_json_promoted_rows"),
            )
            .unwrap();

            let fragment = module.getattr("fragment").unwrap();
            let fragment = extract_template(py, &fragment, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&fragment).unwrap()).unwrap();
            assert_eq!(rendered.text, "{\"label\": \"prefix-suffix\"}");
            assert_eq!(rendered.data, json!({"label": "prefix-suffix"}));

            let promoted = module.getattr("promoted").unwrap();
            let promoted = extract_template(py, &promoted, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&promoted).unwrap()).unwrap();
            assert_eq!(rendered.text, r#"[{"name": "one"}, {"name": "two"}]"#);
            assert_eq!(rendered.data, json!([{"name": "one"}, {"name": "two"}]));
        });
    }

    #[test]
    fn renders_negative_zero_number_shapes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "neg_zero=t'-0'\nneg_zero_float=t'-0.0'\nneg_zero_exp=t'-0E0'\ncombo=t'{{\"a\": [0, -0, -0.0, 1e0, -1E-0]}}'\n"
                ),
                pyo3::ffi::c_str!("test_json_negative_zero_shapes.py"),
                pyo3::ffi::c_str!("test_json_negative_zero_shapes"),
            )
            .unwrap();

            for (name, expected_text, expected_data) in [
                ("neg_zero", "-0", json!(-0.0)),
                ("neg_zero_float", "-0.0", json!(-0.0)),
                ("neg_zero_exp", "-0E0", json!(-0.0)),
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "json_t/json_t_str").unwrap();
                let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
                assert_eq!(rendered.text, expected_text);
                assert_eq!(rendered.data, expected_data);
            }

            let combo = module.getattr("combo").unwrap();
            let combo = extract_template(py, &combo, "json_t/json_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&combo).unwrap()).unwrap();
            assert_eq!(rendered.text, "{\"a\": [0, -0, -0.0, 1e0, -1E-0]}");
            assert_eq!(
                rendered.data,
                serde_json::from_str::<Value>("{\"a\": [0, -0, -0.0, 1e0, -1E-0]}").unwrap()
            );
        });
    }

    #[test]
    fn renders_top_level_keywords_and_empty_collections() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ntop_bool_ws=Template(' \\n true \\t ')\ntop_null_ws=Template(' \\r\\n null \\n')\nempty_object=Template('{ \\n\\t }')\nempty_array=Template('[ \\n\\t ]')\n"
                ),
                pyo3::ffi::c_str!("test_json_top_level_keywords.py"),
                pyo3::ffi::c_str!("test_json_top_level_keywords"),
            )
            .unwrap();

            for (name, expected_text, expected_data) in [
                ("top_bool_ws", "true", json!(true)),
                ("top_null_ws", "null", Value::Null),
                ("empty_object", "{}", json!({})),
                ("empty_array", "[]", json!([])),
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "json_t/json_t_str").unwrap();
                let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
                assert_eq!(rendered.text, expected_text);
                assert_eq!(rendered.data, expected_data);
            }
        });
    }

    #[test]
    fn renders_escape_unicode_and_keyword_text_shapes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ntop_bool_ws=Template(' \\n true \\t ')\ntop_null_ws=Template(' \\r\\n null \\n')\narray_with_line_sep=t'[\"\\\\u2028\", \"\\\\u2029\"]'\nunicode_mix_nested_obj=Template('{\"x\": {\"a\": \"\\\\u005C\", \"b\": \"\\\\u00DF\", \"c\": \"\\\\u2029\"}}')\nkeyword_array=Template('[true,false,null]')\n"
                ),
                pyo3::ffi::c_str!("test_json_escape_unicode_keyword_shapes.py"),
                pyo3::ffi::c_str!("test_json_escape_unicode_keyword_shapes"),
            )
            .unwrap();

            for (name, expected_text, expected_data) in [
                ("top_bool_ws", "true", json!(true)),
                ("top_null_ws", "null", Value::Null),
                (
                    "array_with_line_sep",
                    "[\"\u{2028}\", \"\u{2029}\"]",
                    json!(["\u{2028}", "\u{2029}"]),
                ),
                (
                    "unicode_mix_nested_obj",
                    "{\"x\": {\"a\": \"\\\\\", \"b\": \"ß\", \"c\": \"\u{2029}\"}}",
                    json!({"x": {"a": "\\", "b": "ß", "c": "\u{2029}"}}),
                ),
                (
                    "keyword_array",
                    "[true, false, null]",
                    json!([true, false, null]),
                ),
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "json_t/json_t_str").unwrap();
                let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
                assert_eq!(rendered.text, expected_text, "{name}");
                assert_eq!(rendered.data, expected_data, "{name}");
            }
        });
    }

    #[test]
    fn test_parse_rendered_json_surfaces_parse_failures() {
        let err = parse_rendered_json("{\"a\":,}").expect_err("expected JSON parse failure");
        assert_eq!(err.kind, ErrorKind::Parse);
        assert!(err.message.contains("Rendered JSON could not be reparsed"));
    }
}
