use tstring_syntax::{
    BackendError, BackendResult, NormalizedDate, NormalizedDocument, NormalizedEntry,
    NormalizedFloat, NormalizedKey, NormalizedLocalDateTime, NormalizedOffsetDateTime,
    NormalizedStream, NormalizedTemporal, NormalizedTime, NormalizedValue, SourcePosition,
    SourceSpan, StreamItem, TemplateInput,
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TomlProfile {
    V1_0,
    V1_1,
}

impl TomlProfile {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V1_0 => "1.0",
            Self::V1_1 => "1.1",
        }
    }

    #[must_use]
    const fn allows_missing_seconds(self) -> bool {
        matches!(self, Self::V1_1)
    }

    #[must_use]
    const fn allows_inline_table_newlines(self) -> bool {
        matches!(self, Self::V1_1)
    }

    #[must_use]
    const fn allows_extended_basic_string_escapes(self) -> bool {
        matches!(self, Self::V1_1)
    }
}

impl Default for TomlProfile {
    fn default() -> Self {
        Self::V1_1
    }
}

impl std::str::FromStr for TomlProfile {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "1.0" => Ok(Self::V1_0),
            "1.1" => Ok(Self::V1_1),
            other => Err(format!(
                "Unsupported TOML profile {other:?}. Supported profiles: \"1.0\", \"1.1\"."
            )),
        }
    }
}

#[derive(Clone, Debug)]
pub struct TomlInterpolationNode {
    pub span: SourceSpan,
    pub interpolation_index: usize,
    pub role: String,
}

#[derive(Clone, Debug)]
pub struct TomlStringChunkNode {
    pub span: SourceSpan,
    pub value: String,
}

#[derive(Clone, Debug)]
pub enum TomlStringPart {
    Chunk(TomlStringChunkNode),
    Interpolation(TomlInterpolationNode),
}

#[derive(Clone, Debug)]
pub struct TomlStringNode {
    pub span: SourceSpan,
    pub style: String,
    pub chunks: Vec<TomlStringPart>,
}

#[derive(Clone, Debug)]
pub struct TomlLiteralNode {
    pub span: SourceSpan,
    pub source: String,
    pub value: toml::Value,
}

#[derive(Clone, Debug)]
pub enum TomlKeySegmentValue {
    Bare(String),
    String(TomlStringNode),
    Interpolation(TomlInterpolationNode),
}

#[derive(Clone, Debug)]
pub struct TomlKeySegmentNode {
    pub span: SourceSpan,
    pub value: TomlKeySegmentValue,
    pub bare: bool,
}

#[derive(Clone, Debug)]
pub struct TomlKeyPathNode {
    pub span: SourceSpan,
    pub segments: Vec<TomlKeySegmentNode>,
}

#[derive(Clone, Debug)]
pub struct TomlAssignmentNode {
    pub span: SourceSpan,
    pub key_path: TomlKeyPathNode,
    pub value: TomlValueNode,
}

#[derive(Clone, Debug)]
pub struct TomlTableHeaderNode {
    pub span: SourceSpan,
    pub key_path: TomlKeyPathNode,
}

#[derive(Clone, Debug)]
pub struct TomlArrayTableHeaderNode {
    pub span: SourceSpan,
    pub key_path: TomlKeyPathNode,
}

#[derive(Clone, Debug)]
pub struct TomlArrayNode {
    pub span: SourceSpan,
    pub items: Vec<TomlValueNode>,
}

#[derive(Clone, Debug)]
pub struct TomlInlineTableNode {
    pub span: SourceSpan,
    pub entries: Vec<TomlAssignmentNode>,
}

#[derive(Clone, Debug)]
pub struct TomlDocumentNode {
    pub span: SourceSpan,
    pub statements: Vec<TomlStatementNode>,
}

#[derive(Clone, Debug)]
pub enum TomlValueNode {
    String(TomlStringNode),
    Literal(TomlLiteralNode),
    Interpolation(TomlInterpolationNode),
    Array(TomlArrayNode),
    InlineTable(TomlInlineTableNode),
}

#[derive(Clone, Debug)]
pub enum TomlStatementNode {
    Assignment(TomlAssignmentNode),
    TableHeader(TomlTableHeaderNode),
    ArrayTableHeader(TomlArrayTableHeaderNode),
}

pub struct TomlParser {
    items: Vec<StreamItem>,
    index: usize,
    inline_table_depth: usize,
    profile: TomlProfile,
    literal_materialization: LiteralMaterialization,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LiteralMaterialization {
    SharedHelper,
    Direct,
}

impl TomlParser {
    #[must_use]
    pub fn new(template: &TemplateInput, profile: TomlProfile) -> Self {
        Self::new_with_materialization(template, profile, LiteralMaterialization::SharedHelper)
    }

    #[must_use]
    fn new_with_materialization(
        template: &TemplateInput,
        profile: TomlProfile,
        literal_materialization: LiteralMaterialization,
    ) -> Self {
        Self {
            items: template.flatten(),
            index: 0,
            inline_table_depth: 0,
            profile,
            literal_materialization,
        }
    }

    pub fn parse(&mut self) -> BackendResult<TomlDocumentNode> {
        let start = self.mark();
        let mut statements = Vec::new();
        self.skip_document_junk();
        while self.current_kind() != "eof" {
            if self.starts_with("[[") {
                statements.push(TomlStatementNode::ArrayTableHeader(
                    self.parse_array_table_header()?,
                ));
            } else if self.current_char() == Some('[') {
                statements.push(TomlStatementNode::TableHeader(self.parse_table_header()?));
            } else {
                statements.push(TomlStatementNode::Assignment(self.parse_assignment()?));
            }
            self.skip_line_suffix()?;
            self.skip_document_junk();
        }
        Ok(TomlDocumentNode {
            span: self.span_from(start),
            statements,
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
        BackendError::parse_at("toml.parse", message, Some(self.current().span().clone()))
    }

    fn advance(&mut self) {
        if self.current_kind() != "eof" {
            self.index += 1;
        }
    }

    fn starts_with(&self, text: &str) -> bool {
        let mut probe = self.index;
        for expected in text.chars() {
            match &self.items[probe] {
                StreamItem::Char { ch, .. } if *ch == expected => probe += 1,
                _ => return false,
            }
        }
        true
    }

    fn skip_horizontal_space(&mut self) {
        while matches!(self.current_char(), Some(' ' | '\t')) {
            self.advance();
        }
    }

    fn skip_document_junk(&mut self) {
        loop {
            self.skip_horizontal_space();
            match self.current_char() {
                Some('#') => self.skip_comment(),
                Some('\n') => self.advance(),
                Some('\r') if self.peek_char(1) == Some('\n') => {
                    self.advance();
                    self.advance();
                }
                _ => return,
            }
        }
    }

    fn skip_comment(&mut self) {
        while !matches!(self.current_char(), None | Some('\n')) {
            if self
                .current_char()
                .is_some_and(|ch| is_disallowed_toml_control(ch, false))
            {
                break;
            }
            self.advance();
        }
    }

    fn skip_line_suffix(&mut self) -> BackendResult<()> {
        self.skip_horizontal_space();
        if self.current_char() == Some('#') {
            self.skip_comment();
        }
        if self.current_char() == Some('\n') {
            self.advance();
            return Ok(());
        }
        if self.current_char() == Some('\r') && self.peek_char(1) == Some('\n') {
            self.advance();
            self.advance();
            return Ok(());
        }
        if self.current_kind() != "eof" {
            return Err(self.error("Unexpected trailing TOML content on the line."));
        }
        Ok(())
    }

    fn consume_char(&mut self, expected: char) -> BackendResult<()> {
        if self.current_char() != Some(expected) {
            return Err(self.error(format!("Expected {expected:?} in TOML template.")));
        }
        self.advance();
        Ok(())
    }

    fn parse_assignment(&mut self) -> BackendResult<TomlAssignmentNode> {
        let start = self.mark();
        let key_path = self.parse_key_path(false)?;
        self.skip_horizontal_space();
        self.consume_char('=')?;
        let value = self.parse_value("line")?;
        Ok(TomlAssignmentNode {
            span: self.span_from(start),
            key_path,
            value,
        })
    }

    fn parse_table_header(&mut self) -> BackendResult<TomlTableHeaderNode> {
        let start = self.mark();
        self.consume_char('[')?;
        let key_path = self.parse_key_path(true)?;
        self.consume_char(']')?;
        Ok(TomlTableHeaderNode {
            span: self.span_from(start),
            key_path,
        })
    }

    fn parse_array_table_header(&mut self) -> BackendResult<TomlArrayTableHeaderNode> {
        let start = self.mark();
        self.consume_char('[')?;
        self.consume_char('[')?;
        let key_path = self.parse_key_path(true)?;
        self.consume_char(']')?;
        self.consume_char(']')?;
        Ok(TomlArrayTableHeaderNode {
            span: self.span_from(start),
            key_path,
        })
    }

    fn parse_key_path(&mut self, header: bool) -> BackendResult<TomlKeyPathNode> {
        let start = self.mark();
        let mut segments = vec![self.parse_key_segment()?];
        loop {
            self.skip_horizontal_space();
            if self.current_char() != Some('.') {
                break;
            }
            self.advance();
            self.skip_horizontal_space();
            segments.push(self.parse_key_segment()?);
        }
        if header {
            self.skip_horizontal_space();
        }
        Ok(TomlKeyPathNode {
            span: self.span_from(start),
            segments,
        })
    }

    fn parse_key_segment(&mut self) -> BackendResult<TomlKeySegmentNode> {
        self.skip_horizontal_space();
        let start = self.mark();
        if self.current_kind() == "interpolation" {
            return Ok(TomlKeySegmentNode {
                span: self.span_from(start),
                value: TomlKeySegmentValue::Interpolation(self.consume_interpolation("key")?),
                bare: false,
            });
        }
        if self.starts_with("\"\"\"") {
            return Err(self.error("TOML v1.0 quoted keys cannot be multiline strings."));
        }
        if self.starts_with("'''") {
            return Err(self.error("TOML v1.0 quoted keys cannot be multiline strings."));
        }
        if self.current_char() == Some('"') {
            return Ok(TomlKeySegmentNode {
                span: self.span_from(start),
                value: TomlKeySegmentValue::String(self.parse_string("basic")?),
                bare: false,
            });
        }
        if self.current_char() == Some('\'') {
            return Ok(TomlKeySegmentNode {
                span: self.span_from(start),
                value: TomlKeySegmentValue::String(self.parse_string("literal")?),
                bare: false,
            });
        }
        let bare = self.collect_bare_key();
        if bare.is_empty() {
            return Err(self.error("Expected a TOML key segment."));
        }
        Ok(TomlKeySegmentNode {
            span: self.span_from(start),
            value: TomlKeySegmentValue::Bare(bare),
            bare: true,
        })
    }

    fn collect_bare_key(&mut self) -> String {
        let mut value = String::new();
        while matches!(
            self.current_char(),
            Some('A'..='Z' | 'a'..='z' | '0'..='9' | '_' | '-')
        ) {
            value.push(self.current_char().unwrap_or_default());
            self.advance();
        }
        value
    }

    fn parse_value(&mut self, context: &str) -> BackendResult<TomlValueNode> {
        self.skip_value_space(context);
        if self.current_kind() == "interpolation" {
            let interpolation = self.consume_interpolation("value")?;
            if self.starts_value_terminator(context) {
                return Ok(TomlValueNode::Interpolation(interpolation));
            }
            return Err(self.error("Whole-value TOML interpolations cannot have bare suffix text."));
        }
        if self.current_char() == Some('[') {
            return Ok(TomlValueNode::Array(self.parse_array()?));
        }
        if self.current_char() == Some('{') {
            return Ok(TomlValueNode::InlineTable(self.parse_inline_table()?));
        }
        if self.starts_with("\"\"\"") {
            return Ok(TomlValueNode::String(self.parse_string("multiline_basic")?));
        }
        if self.starts_with("'''") {
            return Ok(TomlValueNode::String(
                self.parse_string("multiline_literal")?,
            ));
        }
        if self.current_char() == Some('"') {
            return Ok(TomlValueNode::String(self.parse_string("basic")?));
        }
        if self.current_char() == Some('\'') {
            return Ok(TomlValueNode::String(self.parse_string("literal")?));
        }
        Ok(TomlValueNode::Literal(self.parse_literal(context)?))
    }

    fn skip_value_space(&mut self, context: &str) {
        loop {
            while matches!(self.current_char(), Some(' ' | '\t')) {
                self.advance();
            }
            if self.current_char() == Some('#') {
                if self.inline_table_depth > 0
                    && context != "array"
                    && !self.profile.allows_inline_table_newlines()
                {
                    return;
                }
                self.skip_comment();
                continue;
            }
            if self.current_char() == Some('\n') {
                if self.inline_table_depth > 0
                    && context != "array"
                    && !self.profile.allows_inline_table_newlines()
                {
                    return;
                }
                if context == "line" {
                    return;
                }
                self.advance();
                continue;
            }
            if self.current_char() == Some('\r') && self.peek_char(1) == Some('\n') {
                if context == "line"
                    || self.inline_table_depth > 0
                        && context != "array"
                        && !self.profile.allows_inline_table_newlines()
                {
                    return;
                }
                self.advance();
                self.advance();
                continue;
            }
            if self.inline_table_depth > 0
                && context != "array"
                && !self.profile.allows_inline_table_newlines()
            {
                return;
            }
            return;
        }
    }

    fn starts_value_terminator(&self, context: &str) -> bool {
        let mut probe = self.index;
        while matches!(self.items[probe].char(), Some(' ' | '\t' | '\r')) {
            probe += 1;
        }
        let item = &self.items[probe];
        if self.inline_table_depth > 0 {
            return match context {
                "array" => matches!(
                    item,
                    StreamItem::Eof { .. }
                        | StreamItem::Char {
                            ch: ',' | ']' | '#',
                            ..
                        }
                ),
                _ if self.profile.allows_inline_table_newlines() => matches!(
                    item,
                    StreamItem::Eof { .. } | StreamItem::Char { ch: ',' | '}', .. }
                ),
                _ => matches!(
                    item,
                    StreamItem::Eof { .. }
                        | StreamItem::Char {
                            ch: ',' | '}' | '#' | '\n' | '\r',
                            ..
                        }
                ),
            };
        }
        if context == "line" {
            matches!(
                item,
                StreamItem::Eof { .. } | StreamItem::Char { ch: '#' | '\n', .. }
            )
        } else {
            matches!(
                item,
                StreamItem::Eof { .. }
                    | StreamItem::Char {
                        ch: ',' | ']' | '}' | '#' | '\n',
                        ..
                    }
            )
        }
    }

    fn parse_array(&mut self) -> BackendResult<TomlArrayNode> {
        let start = self.mark();
        self.consume_char('[')?;
        let mut items = Vec::new();
        self.skip_value_space("array");
        if self.current_char() == Some(']') {
            self.advance();
            return Ok(TomlArrayNode {
                span: self.span_from(start),
                items,
            });
        }
        loop {
            items.push(self.parse_value("array")?);
            self.skip_value_space("array");
            if self.current_char() == Some(']') {
                self.advance();
                break;
            }
            self.consume_char(',')?;
            self.skip_value_space("array");
            if self.current_char() == Some(']') {
                self.advance();
                break;
            }
        }
        Ok(TomlArrayNode {
            span: self.span_from(start),
            items,
        })
    }

    fn parse_inline_table(&mut self) -> BackendResult<TomlInlineTableNode> {
        let start = self.mark();
        self.consume_char('{')?;
        self.inline_table_depth += 1;
        let mut entries = Vec::new();
        self.skip_value_space("inline");
        if self.current_char() == Some('}') {
            self.advance();
            self.inline_table_depth -= 1;
            return Ok(TomlInlineTableNode {
                span: self.span_from(start),
                entries,
            });
        }
        loop {
            let entry_start = self.mark();
            let key_path = self.parse_key_path(false)?;
            self.skip_horizontal_space();
            self.consume_char('=')?;
            let value = self.parse_value("inline")?;
            entries.push(TomlAssignmentNode {
                span: self.span_from(entry_start),
                key_path,
                value,
            });
            self.skip_value_space("inline");
            if self.current_char() == Some('}') {
                self.advance();
                break;
            }
            self.consume_char(',')?;
            self.skip_value_space("inline");
            if self.current_char() == Some('}') {
                if !self.profile.allows_inline_table_newlines() {
                    return Err(
                        self.error("Trailing commas are not permitted in TOML 1.0 inline tables.")
                    );
                }
                self.advance();
                break;
            }
        }
        self.inline_table_depth -= 1;
        Ok(TomlInlineTableNode {
            span: self.span_from(start),
            entries,
        })
    }

    fn parse_string(&mut self, style: &str) -> BackendResult<TomlStringNode> {
        let start = self.mark();
        match style {
            "basic" => {
                self.consume_char('"')?;
                self.parse_basic_like_string(start, style, false)
            }
            "multiline_basic" => {
                self.consume_char('"')?;
                self.consume_char('"')?;
                self.consume_char('"')?;
                self.consume_multiline_opening_newline();
                self.parse_basic_like_string(start, style, true)
            }
            "literal" => {
                self.consume_char('\'')?;
                self.parse_literal_like_string(start, style, false)
            }
            _ => {
                self.consume_char('\'')?;
                self.consume_char('\'')?;
                self.consume_char('\'')?;
                self.consume_multiline_opening_newline();
                self.parse_literal_like_string(start, style, true)
            }
        }
    }

    fn consume_multiline_opening_newline(&mut self) {
        if self.current_char() == Some('\r') {
            self.advance();
        }
        if self.current_char() == Some('\n') {
            self.advance();
        }
    }

    fn parse_basic_like_string(
        &mut self,
        start: SourcePosition,
        style: &str,
        multiline: bool,
    ) -> BackendResult<TomlStringNode> {
        let mut chunks = Vec::new();
        let mut buffer = String::new();
        loop {
            if multiline && self.starts_with("\"\"\"") {
                let quote_run = self.count_consecutive_chars('"');
                if quote_run == 4 || quote_run == 5 {
                    for _ in 0..(quote_run - 3) {
                        buffer.push('"');
                        self.advance();
                    }
                    continue;
                }
                self.flush_buffer(&mut buffer, &mut chunks);
                self.consume_char('"')?;
                self.consume_char('"')?;
                self.consume_char('"')?;
                break;
            }
            if !multiline && self.current_char() == Some('"') {
                self.flush_buffer(&mut buffer, &mut chunks);
                self.advance();
                break;
            }
            if !multiline && matches!(self.current_char(), Some('\r' | '\n')) {
                return Err(self.error("TOML single-line basic strings cannot contain newlines."));
            }
            if self.current_kind() == "eof" {
                return Err(self.error("Unterminated TOML basic string."));
            }
            if self.current_kind() == "interpolation" {
                self.flush_buffer(&mut buffer, &mut chunks);
                chunks.push(TomlStringPart::Interpolation(
                    self.consume_interpolation("string_fragment")?,
                ));
                continue;
            }
            if multiline && self.current_char() == Some('\\') && self.starts_multiline_escape() {
                self.consume_multiline_escape();
                continue;
            }
            if self.current_char() == Some('\\') {
                buffer.push(self.parse_basic_escape()?);
                continue;
            }
            if multiline && self.current_char() == Some('\r') && self.peek_char(1) == Some('\n') {
                buffer.push('\n');
                self.advance();
                self.advance();
                continue;
            }
            if multiline && self.current_char() == Some('\n') {
                buffer.push('\n');
                self.advance();
                continue;
            }
            if let Some(ch) = self.current_char() {
                if is_disallowed_toml_control(ch, multiline) {
                    return Err(self.error("Invalid TOML character in basic string."));
                }
                buffer.push(ch);
                self.advance();
                continue;
            }
            return Err(self.error("Unterminated TOML basic string."));
        }
        Ok(TomlStringNode {
            span: self.span_from(start),
            style: style.to_owned(),
            chunks,
        })
    }

    fn starts_multiline_escape(&self) -> bool {
        let mut probe = self.index + 1;
        while self
            .items
            .get(probe)
            .and_then(StreamItem::char)
            .is_some_and(|ch| matches!(ch, ' ' | '\t'))
        {
            probe += 1;
        }
        if self
            .items
            .get(probe)
            .and_then(StreamItem::char)
            .is_some_and(|ch| ch == '\r')
        {
            probe += 1;
        }
        self.items
            .get(probe)
            .and_then(StreamItem::char)
            .is_some_and(|ch| ch == '\n')
    }

    fn consume_multiline_escape(&mut self) {
        self.advance();
        while matches!(self.current_char(), Some(' ' | '\t')) {
            self.advance();
        }
        if self.current_char() == Some('\r') {
            self.advance();
        }
        if self.current_char() == Some('\n') {
            self.advance();
        }
        while matches!(self.current_char(), Some(' ' | '\t' | '\n' | '\r')) {
            self.advance();
        }
    }

    fn parse_literal_like_string(
        &mut self,
        start: SourcePosition,
        style: &str,
        multiline: bool,
    ) -> BackendResult<TomlStringNode> {
        let mut chunks = Vec::new();
        let mut buffer = String::new();
        loop {
            if multiline && self.starts_with("'''") {
                let quote_run = self.count_consecutive_chars('\'');
                if quote_run == 4 || quote_run == 5 {
                    for _ in 0..(quote_run - 3) {
                        buffer.push('\'');
                        self.advance();
                    }
                    continue;
                }
                self.flush_buffer(&mut buffer, &mut chunks);
                self.consume_char('\'')?;
                self.consume_char('\'')?;
                self.consume_char('\'')?;
                break;
            }
            if !multiline && self.current_char() == Some('\'') {
                self.flush_buffer(&mut buffer, &mut chunks);
                self.advance();
                break;
            }
            if !multiline && matches!(self.current_char(), Some('\r' | '\n')) {
                return Err(self.error("TOML single-line literal strings cannot contain newlines."));
            }
            if self.current_kind() == "eof" {
                return Err(self.error("Unterminated TOML literal string."));
            }
            if self.current_kind() == "interpolation" {
                self.flush_buffer(&mut buffer, &mut chunks);
                chunks.push(TomlStringPart::Interpolation(
                    self.consume_interpolation("string_fragment")?,
                ));
                continue;
            }
            if multiline && self.current_char() == Some('\r') && self.peek_char(1) == Some('\n') {
                buffer.push('\n');
                self.advance();
                self.advance();
                continue;
            }
            if multiline && self.current_char() == Some('\n') {
                buffer.push('\n');
                self.advance();
                continue;
            }
            if let Some(ch) = self.current_char() {
                if is_disallowed_toml_control(ch, multiline) {
                    return Err(self.error("Invalid TOML character in literal string."));
                }
                buffer.push(ch);
                self.advance();
                continue;
            }
            return Err(self.error("Unterminated TOML literal string."));
        }
        Ok(TomlStringNode {
            span: self.span_from(start),
            style: style.to_owned(),
            chunks,
        })
    }

    fn parse_basic_escape(&mut self) -> BackendResult<char> {
        self.consume_char('\\')?;
        let ch = self
            .current_char()
            .ok_or_else(|| self.error("Incomplete TOML escape sequence."))?;
        self.advance();
        let mapped = match ch {
            '"' => Some('"'),
            '\\' => Some('\\'),
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
        if ch == 'u' {
            let digits = self.collect_exact_chars(4)?;
            let codepoint = u32::from_str_radix(&digits, 16)
                .map_err(|_| self.error("Invalid TOML escape sequence."))?;
            return char::from_u32(codepoint)
                .ok_or_else(|| self.error("Invalid TOML escape sequence."));
        }
        if ch == 'U' {
            let digits = self.collect_exact_chars(8)?;
            let codepoint = u32::from_str_radix(&digits, 16)
                .map_err(|_| self.error("Invalid TOML escape sequence."))?;
            return char::from_u32(codepoint)
                .ok_or_else(|| self.error("Invalid TOML escape sequence."));
        }
        if self.profile.allows_extended_basic_string_escapes() && ch == 'e' {
            return Ok('\u{001b}');
        }
        if self.profile.allows_extended_basic_string_escapes() && ch == 'x' {
            let digits = self.collect_exact_chars(2)?;
            let codepoint = u32::from_str_radix(&digits, 16)
                .map_err(|_| self.error("Invalid TOML escape sequence."))?;
            return char::from_u32(codepoint)
                .ok_or_else(|| self.error("Invalid TOML escape sequence."));
        }
        Err(self.error("Invalid TOML escape sequence."))
    }

    fn collect_exact_chars(&mut self, count: usize) -> BackendResult<String> {
        let mut chars = String::new();
        for _ in 0..count {
            let ch = self
                .current_char()
                .ok_or_else(|| self.error("Unexpected end of TOML escape sequence."))?;
            chars.push(ch);
            self.advance();
        }
        Ok(chars)
    }

    fn count_consecutive_chars(&self, ch: char) -> usize {
        let mut probe = self.index;
        let mut count = 0usize;
        while self.items.get(probe).and_then(StreamItem::char) == Some(ch) {
            count += 1;
            probe += 1;
        }
        count
    }

    fn peek_char(&self, offset: usize) -> Option<char> {
        self.items
            .get(self.index + offset)
            .and_then(StreamItem::char)
    }

    fn flush_buffer(&self, buffer: &mut String, chunks: &mut Vec<TomlStringPart>) {
        if buffer.is_empty() {
            return;
        }
        chunks.push(TomlStringPart::Chunk(TomlStringChunkNode {
            span: SourceSpan::point(0, 0),
            value: std::mem::take(buffer),
        }));
    }

    fn parse_literal(&mut self, context: &str) -> BackendResult<TomlLiteralNode> {
        let start = self.mark();
        let mut source = String::new();
        while self.current_kind() != "eof" {
            if self.starts_value_terminator(context) {
                break;
            }
            if self.current_kind() == "interpolation" {
                return Err(
                    self.error("TOML bare literals cannot contain fragment interpolations.")
                );
            }
            if let Some(ch) = self.current_char() {
                source.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        source = source.trim_end().to_owned();
        if source.is_empty() {
            return Err(self.error("Expected a TOML value."));
        }
        let value = match self.literal_materialization {
            LiteralMaterialization::SharedHelper => materialize_value_source(self.profile, &source)
                .map_err(|message| self.error(message))?,
            LiteralMaterialization::Direct => {
                materialize_value_source_direct(self.profile, &source)
                    .map_err(|message| self.error(message))?
            }
        };
        Ok(TomlLiteralNode {
            span: self.span_from(start),
            source,
            value,
        })
    }

    fn consume_interpolation(&mut self, role: &str) -> BackendResult<TomlInterpolationNode> {
        let (interpolation_index, span) = match self.current() {
            StreamItem::Interpolation {
                interpolation_index,
                span,
                ..
            } => (*interpolation_index, span.clone()),
            _ => return Err(self.error("Expected an interpolation.")),
        };
        self.advance();
        Ok(TomlInterpolationNode {
            span,
            interpolation_index,
            role: role.to_owned(),
        })
    }
}

fn is_disallowed_toml_control(ch: char, multiline: bool) -> bool {
    if ch == '\t' {
        return false;
    }
    if multiline && ch == '\n' {
        return false;
    }
    matches!(
        ch,
        '\u{0000}'..='\u{0008}'
            | '\u{000b}'
            | '\u{000c}'
            | '\u{000e}'..='\u{001f}'
            | '\u{007f}'
    )
}

fn is_v1_datetime_without_seconds(source: &str) -> bool {
    if !source.chars().all(|ch| !ch.is_whitespace()) {
        return false;
    }
    if let Some(time_part) = source.split('T').nth(1) {
        return is_time_without_seconds(time_part);
    }
    source.matches(':').count() == 1 && is_time_without_seconds(source)
}

fn is_time_without_seconds(value: &str) -> bool {
    if value.len() < 5 {
        return false;
    }
    let bytes = value.as_bytes();
    if !(bytes[0].is_ascii_digit()
        && bytes[1].is_ascii_digit()
        && bytes[2] == b':'
        && bytes[3].is_ascii_digit()
        && bytes[4].is_ascii_digit())
    {
        return false;
    }
    !matches!(bytes.get(5), Some(b':'))
}

fn materialize_value_source_direct(
    profile: TomlProfile,
    source_text: &str,
) -> Result<toml::Value, String> {
    if !profile.allows_missing_seconds() && is_v1_datetime_without_seconds(source_text) {
        return Err("Invalid TOML literal: missing seconds in time value.".to_owned());
    }
    let table: toml::Table = toml::from_str(&format!("value = {source_text}"))
        .map_err(|err| format!("Invalid TOML literal: {err}"))?;
    table
        .get("value")
        .cloned()
        .ok_or_else(|| "Expected a TOML value.".to_owned())
}

pub fn materialize_value_source(
    profile: TomlProfile,
    source_text: &str,
) -> Result<toml::Value, String> {
    // The template parser enforces profile-specific grammar boundaries first.
    // We still materialize with `toml::from_str` afterwards so literals and
    // formatted interpolation payloads share the same `toml` crate semantics.
    let template = TemplateInput::from_segments(vec![tstring_syntax::TemplateSegment::StaticText(
        format!("value = {source_text}"),
    )]);
    TomlParser::new_with_materialization(&template, profile, LiteralMaterialization::Direct)
        .parse()
        .map_err(|err| err.message.clone())?;
    materialize_value_source_direct(profile, source_text)
}

pub fn parse_template_with_profile(
    template: &TemplateInput,
    profile: TomlProfile,
) -> BackendResult<TomlDocumentNode> {
    let items = template.flatten();
    for window in items.windows(2) {
        if window[0].char() == Some('\r') && window[1].char() != Some('\n') {
            return Err(BackendError::parse_at(
                "toml.parse",
                "Bare carriage returns are not valid in TOML input.",
                Some(window[0].span().clone()),
            ));
        }
    }
    TomlParser::new(template, profile).parse()
}

pub fn parse_template(template: &TemplateInput) -> BackendResult<TomlDocumentNode> {
    parse_template_with_profile(template, TomlProfile::default())
}

pub fn check_template_with_profile(
    template: &TemplateInput,
    profile: TomlProfile,
) -> BackendResult<()> {
    parse_template_with_profile(template, profile).map(|_| ())
}

pub fn check_template(template: &TemplateInput) -> BackendResult<()> {
    check_template_with_profile(template, TomlProfile::default())
}

pub fn format_template_with_profile(
    template: &TemplateInput,
    profile: TomlProfile,
) -> BackendResult<String> {
    let document = parse_template_with_profile(template, profile)?;
    format_toml_document(template, &document)
}

pub fn format_template(template: &TemplateInput) -> BackendResult<String> {
    format_template_with_profile(template, TomlProfile::default())
}

pub fn normalize_document_with_profile(
    value: &toml::Value,
    _profile: TomlProfile,
) -> BackendResult<NormalizedStream> {
    Ok(NormalizedStream::new(vec![NormalizedDocument::Value(
        normalize_value(value)?,
    )]))
}

pub fn normalize_document(value: &toml::Value) -> BackendResult<NormalizedStream> {
    normalize_document_with_profile(value, TomlProfile::default())
}

pub fn normalize_value(value: &toml::Value) -> BackendResult<NormalizedValue> {
    match value {
        toml::Value::String(value) => Ok(NormalizedValue::String(value.clone())),
        toml::Value::Integer(value) => Ok(NormalizedValue::Integer((*value).into())),
        toml::Value::Float(value) => Ok(NormalizedValue::Float(normalize_float(*value))),
        toml::Value::Boolean(value) => Ok(NormalizedValue::Bool(*value)),
        toml::Value::Datetime(value) => Ok(NormalizedValue::Temporal(normalize_datetime(value)?)),
        toml::Value::Array(values) => values
            .iter()
            .map(normalize_value)
            .collect::<BackendResult<Vec<_>>>()
            .map(NormalizedValue::Sequence),
        toml::Value::Table(values) => values
            .iter()
            .map(|(key, value)| {
                Ok(NormalizedEntry {
                    key: NormalizedKey::String(key.clone()),
                    value: normalize_value(value)?,
                })
            })
            .collect::<BackendResult<Vec<_>>>()
            .map(NormalizedValue::Mapping),
    }
}

fn format_toml_document(
    template: &TemplateInput,
    node: &TomlDocumentNode,
) -> BackendResult<String> {
    node.statements
        .iter()
        .map(|statement| format_toml_statement(template, statement))
        .collect::<BackendResult<Vec<_>>>()
        .map(|statements| statements.join("\n"))
}

fn format_toml_statement(
    template: &TemplateInput,
    node: &TomlStatementNode,
) -> BackendResult<String> {
    match node {
        TomlStatementNode::Assignment(node) => Ok(format!(
            "{} = {}",
            format_key_path(template, &node.key_path)?,
            format_toml_value(template, &node.value)?
        )),
        TomlStatementNode::TableHeader(node) => {
            Ok(format!("[{}]", format_key_path(template, &node.key_path)?))
        }
        TomlStatementNode::ArrayTableHeader(node) => Ok(format!(
            "[[{}]]",
            format_key_path(template, &node.key_path)?
        )),
    }
}

fn format_key_path(template: &TemplateInput, node: &TomlKeyPathNode) -> BackendResult<String> {
    node.segments
        .iter()
        .map(|segment| format_key_segment(template, segment))
        .collect::<BackendResult<Vec<_>>>()
        .map(|segments| segments.join("."))
}

fn format_key_segment(
    template: &TemplateInput,
    node: &TomlKeySegmentNode,
) -> BackendResult<String> {
    match &node.value {
        TomlKeySegmentValue::Bare(value) => {
            if node.bare && is_bare_key(value) {
                Ok(value.clone())
            } else {
                Ok(render_basic_string(value))
            }
        }
        TomlKeySegmentValue::String(value) => format_toml_string(template, value),
        TomlKeySegmentValue::Interpolation(value) => {
            interpolation_raw_source(template, value.interpolation_index, &value.span, "TOML key")
        }
    }
}

fn format_toml_value(template: &TemplateInput, node: &TomlValueNode) -> BackendResult<String> {
    match node {
        TomlValueNode::String(node) => format_toml_string(template, node),
        TomlValueNode::Literal(node) => Ok(node.source.clone()),
        TomlValueNode::Interpolation(node) => {
            interpolation_raw_source(template, node.interpolation_index, &node.span, "TOML value")
        }
        TomlValueNode::Array(node) => node
            .items
            .iter()
            .map(|item| format_toml_value(template, item))
            .collect::<BackendResult<Vec<_>>>()
            .map(|items| format!("[{}]", items.join(", "))),
        TomlValueNode::InlineTable(node) => node
            .entries
            .iter()
            .map(|entry| {
                Ok(format!(
                    "{} = {}",
                    format_key_path(template, &entry.key_path)?,
                    format_toml_value(template, &entry.value)?
                ))
            })
            .collect::<BackendResult<Vec<_>>>()
            .map(|entries| {
                if entries.is_empty() {
                    "{}".to_owned()
                } else {
                    format!("{{ {} }}", entries.join(", "))
                }
            }),
    }
}

fn format_toml_string(template: &TemplateInput, node: &TomlStringNode) -> BackendResult<String> {
    let mut rendered = String::new();
    for chunk in &node.chunks {
        match chunk {
            TomlStringPart::Chunk(chunk) => rendered.push_str(&chunk.value),
            TomlStringPart::Interpolation(node) => rendered.push_str(&interpolation_raw_source(
                template,
                node.interpolation_index,
                &node.span,
                "TOML string fragment",
            )?),
        }
    }
    Ok(render_basic_string(&rendered))
}

fn interpolation_raw_source(
    template: &TemplateInput,
    interpolation_index: usize,
    span: &SourceSpan,
    context: &str,
) -> BackendResult<String> {
    template
        .interpolation_raw_source(interpolation_index)
        .map(str::to_owned)
        .ok_or_else(|| {
            let expression = template.interpolation(interpolation_index).map_or_else(
                || format!("slot {interpolation_index}"),
                |value| value.expression_label().to_owned(),
            );
            BackendError::semantic_at(
                "toml.format",
                format!(
                    "Cannot format {context} interpolation {expression:?} without raw source text."
                ),
                Some(span.clone()),
            )
        })
}

fn is_bare_key(value: &str) -> bool {
    value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn render_basic_string(value: &str) -> String {
    let mut rendered = String::with_capacity(value.len() + 2);
    rendered.push('"');
    for ch in value.chars() {
        match ch {
            '\u{0008}' => rendered.push_str("\\b"),
            '\t' => rendered.push_str("\\t"),
            '\n' => rendered.push_str("\\n"),
            '\u{000c}' => rendered.push_str("\\f"),
            '\r' => rendered.push_str("\\r"),
            '"' => rendered.push_str("\\\""),
            '\\' => rendered.push_str("\\\\"),
            '\u{0000}'..='\u{001f}' | '\u{007f}' => {
                rendered.push_str(&format!("\\u{:04X}", ch as u32));
            }
            _ => rendered.push(ch),
        }
    }
    rendered.push('"');
    rendered
}

fn normalize_float(value: f64) -> NormalizedFloat {
    if value.is_nan() {
        return NormalizedFloat::NaN;
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            NormalizedFloat::NegInf
        } else {
            NormalizedFloat::PosInf
        };
    }
    NormalizedFloat::finite(value)
}

fn normalize_datetime(value: &toml::value::Datetime) -> BackendResult<NormalizedTemporal> {
    match (&value.date, &value.time, &value.offset) {
        (Some(date), Some(time), Some(offset)) => Ok(NormalizedTemporal::OffsetDateTime(
            NormalizedOffsetDateTime {
                date: normalize_date(*date),
                time: normalize_time(*time),
                offset_minutes: match offset {
                    toml::value::Offset::Z => 0,
                    toml::value::Offset::Custom { minutes } => *minutes,
                },
            },
        )),
        (Some(date), Some(time), None) => {
            Ok(NormalizedTemporal::LocalDateTime(NormalizedLocalDateTime {
                date: normalize_date(*date),
                time: normalize_time(*time),
            }))
        }
        (Some(date), None, None) => Ok(NormalizedTemporal::LocalDate(normalize_date(*date))),
        (None, Some(time), None) => Ok(NormalizedTemporal::LocalTime(normalize_time(*time))),
        _ => Err(BackendError::semantic(format!(
            "Unsupported TOML datetime shape: {value}"
        ))),
    }
}

fn normalize_date(value: toml::value::Date) -> NormalizedDate {
    NormalizedDate {
        year: i32::from(value.year),
        month: value.month,
        day: value.day,
    }
}

fn normalize_time(value: toml::value::Time) -> NormalizedTime {
    NormalizedTime {
        hour: value.hour,
        minute: value.minute,
        second: value.second,
        nanosecond: value.nanosecond,
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_template, TomlKeySegmentValue, TomlStatementNode, TomlValueNode};
    use pyo3::prelude::*;
    use tstring_pyo3_bindings::{extract_template, toml::render_document};
    use tstring_syntax::{BackendError, BackendResult, ErrorKind};

    fn parse_rendered_toml(text: &str) -> BackendResult<toml::Value> {
        toml::from_str(text).map_err(|err| {
            BackendError::parse(format!(
                "Rendered TOML could not be reparsed during test verification: {err}"
            ))
        })
    }

    #[test]
    fn parses_toml_string_families() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("template=t'basic = \"hi-{1}\"\\nliteral = \\'hi-{2}\\''\n"),
                pyo3::ffi::c_str!("test_toml.py"),
                pyo3::ffi::c_str!("test_toml"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
            let document = parse_template(&template).unwrap();
            assert_eq!(document.statements.len(), 2);
            let TomlStatementNode::Assignment(first) = &document.statements[0] else {
                panic!("expected assignment");
            };
            let TomlValueNode::String(first_value) = &first.value else {
                panic!("expected string");
            };
            assert_eq!(first_value.style, "basic");
        });
    }

    #[test]
    fn parses_headers_and_interpolated_key_segments() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "env='prod'\nname='api'\ntemplate=t'[servers.{env}]\\nservice = \"{name}\"\\n[[services]]\\nid = 1\\n'\n"
                ),
                pyo3::ffi::c_str!("test_toml_headers.py"),
                pyo3::ffi::c_str!("test_toml_headers"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
            let document = parse_template(&template).unwrap();

            assert_eq!(document.statements.len(), 4);
            let TomlStatementNode::TableHeader(header) = &document.statements[0] else {
                panic!("expected table header");
            };
            assert_eq!(header.key_path.segments.len(), 2);
            assert!(matches!(
                header.key_path.segments[1].value,
                TomlKeySegmentValue::Interpolation(_)
            ));
            assert!(matches!(
                document.statements[2],
                TomlStatementNode::ArrayTableHeader(_)
            ));
        });
    }

    #[test]
    fn parses_quoted_keys_and_multiline_array_comments() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ntemplate=t'\"a.b\" = 1\\nsite.\"google.com\".value = 2\\nvalue = [\\n  1, # first\\n  2, # second\\n]\\n'\nempty_basic=Template('\"\" = 1\\n')\nempty_literal=Template(\"'' = 1\\n\")\nempty_segment=Template('a.\"\".b = 1\\n')\n"
                ),
                pyo3::ffi::c_str!("test_toml_quoted_keys.py"),
                pyo3::ffi::c_str!("test_toml_quoted_keys"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
            let document = parse_template(&template).unwrap();
            let rendered = render_document(py, &document).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");

            assert_eq!(table["a.b"].as_integer(), Some(1));
            assert_eq!(table["site"]["google.com"]["value"].as_integer(), Some(2));
            assert_eq!(
                table["value"]
                    .as_array()
                    .expect("array")
                    .iter()
                    .filter_map(toml::Value::as_integer)
                    .collect::<Vec<_>>(),
                vec![1, 2]
            );

            let empty_basic = module.getattr("empty_basic").unwrap();
            let empty_basic = extract_template(py, &empty_basic, "toml_t/toml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&empty_basic).unwrap()).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");
            assert_eq!(table[""].as_integer(), Some(1));

            let empty_literal = module.getattr("empty_literal").unwrap();
            let empty_literal = extract_template(py, &empty_literal, "toml_t/toml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&empty_literal).unwrap()).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");
            assert_eq!(table[""].as_integer(), Some(1));

            let empty_segment = module.getattr("empty_segment").unwrap();
            let empty_segment = extract_template(py, &empty_segment, "toml_t/toml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&empty_segment).unwrap()).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");
            assert_eq!(table["a"][""]["b"].as_integer(), Some(1));
        });
    }

    #[test]
    fn renders_temporal_values_and_inline_tables() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from datetime import datetime\nmoment=datetime(2025, 1, 2, 3, 4, 5)\nmeta={'count': 2, 'active': True}\ntemplate=t'when = {moment}\\nmeta = {meta}\\n'\n"
                ),
                pyo3::ffi::c_str!("test_toml_render.py"),
                pyo3::ffi::c_str!("test_toml_render"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
            let document = parse_template(&template).unwrap();
            let rendered = render_document(py, &document).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");

            assert!(rendered.text.contains("2025-01-02T03:04:05"));
            assert_eq!(table["meta"]["count"].as_integer(), Some(2));
            assert_eq!(table["meta"]["active"].as_bool(), Some(true));
        });
    }

    #[test]
    fn rejects_null_like_interpolations() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("missing=None\ntemplate=t'value = {missing}\\n'\n"),
                pyo3::ffi::c_str!("test_toml_error.py"),
                pyo3::ffi::c_str!("test_toml_error"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
            let document = parse_template(&template).unwrap();
            let err = match render_document(py, &document) {
                Ok(_) => panic!("expected TOML render failure"),
                Err(err) => err,
            };

            assert_eq!(err.kind, ErrorKind::Unrepresentable);
            assert!(err.message.contains("TOML has no null"));
        });
    }

    #[test]
    fn trims_multiline_basic_line_end_backslashes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ntrimmed=t'value = \"\"\"\\nalpha\\\\\\n  beta\\n\"\"\"'\ncrlf=Template('value = \"\"\"\\r\\na\\\\\\r\\n  b\\r\\n\"\"\"\\n')\n"
                ),
                pyo3::ffi::c_str!("test_toml_multiline.py"),
                pyo3::ffi::c_str!("test_toml_multiline"),
            )
            .unwrap();
            for (name, expected) in [("trimmed", "alphabeta\n"), ("crlf", "ab\n")] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
                let document = parse_template(&template).unwrap();
                let rendered = render_document(py, &document).unwrap();
                let table = rendered.data.as_table().expect("expected TOML table");
                assert_eq!(table["value"].as_str(), Some(expected));
            }
        });
    }

    #[test]
    fn parses_multiline_strings_with_one_or_two_quotes_before_terminator() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "template=t'value = \"\"\"\"\"\"\"\\none = \"\"\"\"\"\"\"\"\\nliteral = \\'\\'\\'\\'\\'\\'\\'\\nliteral_two = \\'\\'\\'\\'\\'\\'\\'\\'\\n'\n"
                ),
                pyo3::ffi::c_str!("test_toml_quote_run.py"),
                pyo3::ffi::c_str!("test_toml_quote_run"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
            let document = parse_template(&template).unwrap();
            let rendered = render_document(py, &document).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");

            assert_eq!(table["value"].as_str(), Some("\""));
            assert_eq!(table["one"].as_str(), Some("\"\""));
            assert_eq!(table["literal"].as_str(), Some("'"));
            assert_eq!(table["literal_two"].as_str(), Some("''"));
        });
    }

    #[test]
    fn parses_numeric_forms_and_local_datetimes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "template=t'value = 0xDEADBEEF\\nhex_underscore = 0xDEAD_BEEF\\nbinary = 0b1101\\noctal = 0o755\\nunderscored = 1_000_000\\nfloat = +1.0\\nexp = -2e-2\\nlocal = 2024-01-02T03:04:05\\n'\n"
                ),
                pyo3::ffi::c_str!("test_toml_numeric_forms.py"),
                pyo3::ffi::c_str!("test_toml_numeric_forms"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
            let document = parse_template(&template).unwrap();
            let rendered = render_document(py, &document).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");

            assert_eq!(table["value"].as_integer(), Some(3_735_928_559));
            assert_eq!(table["hex_underscore"].as_integer(), Some(3_735_928_559));
            assert_eq!(table["binary"].as_integer(), Some(13));
            assert_eq!(table["octal"].as_integer(), Some(493));
            assert_eq!(table["underscored"].as_integer(), Some(1_000_000));
            assert_eq!(table["float"].as_float(), Some(1.0));
            assert_eq!(table["exp"].as_float(), Some(-0.02));
            assert_eq!(
                table["local"]
                    .as_datetime()
                    .map(std::string::ToString::to_string),
                Some("2024-01-02T03:04:05".to_owned())
            );
        });
    }

    #[test]
    fn parses_empty_strings_and_quoted_empty_table_headers() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nbasic=t'value = \"\"\\n'\nliteral=Template(\"value = ''\\n\")\nheader=Template('[\"\"]\\nvalue = 1\\n')\nheader_subtable=Template('[\"\"]\\nvalue = 1\\n[\"\".inner]\\nname = \"x\"\\n')\nescaped_quote=Template('value = \"\"\"a\\\\\"b\"\"\"\\n')\n"
                ),
                pyo3::ffi::c_str!("test_toml_empty_strings.py"),
                pyo3::ffi::c_str!("test_toml_empty_strings"),
            )
            .unwrap();

            for name in ["basic", "literal"] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
                let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
                let table = rendered.data.as_table().expect("expected TOML table");
                assert_eq!(table["value"].as_str(), Some(""));
            }

            let escaped_quote = module.getattr("escaped_quote").unwrap();
            let escaped_quote = extract_template(py, &escaped_quote, "toml_t/toml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&escaped_quote).unwrap()).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");
            assert_eq!(table["value"].as_str(), Some("a\"b"));

            let header = module.getattr("header").unwrap();
            let header = extract_template(py, &header, "toml_t/toml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&header).unwrap()).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");
            assert_eq!(table[""]["value"].as_integer(), Some(1));

            let header_subtable = module.getattr("header_subtable").unwrap();
            let header_subtable =
                extract_template(py, &header_subtable, "toml_t/toml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&header_subtable).unwrap()).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");
            assert_eq!(table[""]["inner"]["name"].as_str(), Some("x"));
        });
    }

    #[test]
    fn parses_empty_collections_and_quoted_empty_dotted_tables() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nempty_array=Template('value = []\\n')\nempty_inline_table=Template('value = {}\\n')\nquoted_empty_dotted_table=Template('[a.\"\".b]\\nvalue = 1\\n')\nquoted_empty_subsegments=Template('[\"\".\"\".leaf]\\nvalue = 1\\n')\nquoted_empty_leaf_chain=Template('[\"\".\"\".\"leaf\"]\\nvalue = 1\\n')\nmixed_array_tables=Template('[[a]]\\nname = \"x\"\\n[[a]]\\nname = \"y\"\\n')\n"
                ),
                pyo3::ffi::c_str!("test_toml_empty_collections.py"),
                pyo3::ffi::c_str!("test_toml_empty_collections"),
            )
            .unwrap();

            let empty_array = module.getattr("empty_array").unwrap();
            let empty_array = extract_template(py, &empty_array, "toml_t/toml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&empty_array).unwrap()).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");
            assert_eq!(table["value"].as_array().expect("array").len(), 0);

            let empty_inline_table = module.getattr("empty_inline_table").unwrap();
            let empty_inline_table =
                extract_template(py, &empty_inline_table, "toml_t/toml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&empty_inline_table).unwrap()).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");
            assert_eq!(table["value"].as_table().expect("table").len(), 0);

            let quoted_empty_dotted_table = module.getattr("quoted_empty_dotted_table").unwrap();
            let quoted_empty_dotted_table =
                extract_template(py, &quoted_empty_dotted_table, "toml_t/toml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&quoted_empty_dotted_table).unwrap()).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");
            assert_eq!(table["a"][""]["b"]["value"].as_integer(), Some(1));

            let quoted_empty_subsegments = module.getattr("quoted_empty_subsegments").unwrap();
            let quoted_empty_subsegments =
                extract_template(py, &quoted_empty_subsegments, "toml_t/toml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&quoted_empty_subsegments).unwrap()).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");
            assert_eq!(table[""][""]["leaf"]["value"].as_integer(), Some(1));

            let quoted_empty_leaf_chain = module.getattr("quoted_empty_leaf_chain").unwrap();
            let quoted_empty_leaf_chain =
                extract_template(py, &quoted_empty_leaf_chain, "toml_t/toml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&quoted_empty_leaf_chain).unwrap()).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");
            assert_eq!(table[""][""]["leaf"]["value"].as_integer(), Some(1));

            let mixed_array_tables = module.getattr("mixed_array_tables").unwrap();
            let mixed_array_tables =
                extract_template(py, &mixed_array_tables, "toml_t/toml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&mixed_array_tables).unwrap()).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");
            assert_eq!(table["a"].as_array().expect("array").len(), 2);
        });
    }

    #[test]
    fn parses_additional_numeric_and_datetime_forms() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "template=t'plus_int = +1\\nplus_zero = +0\\nplus_zero_float = +0.0\\nzero_float_exp = 0e0\\nplus_zero_float_exp = +0e0\\nplus_zero_fraction_exp = +0.0e0\\nexp_underscore = 1e1_0\\nfrac_underscore = 1_2.3_4\\nlocal_space = 2024-01-02 03:04:05\\nlocal_lower_t = 2024-01-02t03:04:05\\nlocal_date = 2024-01-02\\nlocal_time_fraction = 03:04:05.123456\\narray_of_dates = [2024-01-02, 2024-01-03]\\narray_of_dates_trailing = [2024-01-02, 2024-01-03,]\\nmixed_date_time_array = [2024-01-02, 03:04:05]\\narray_of_local_times = [03:04:05, 03:04:06.123456]\\nnested_array_mixed_dates = [[2024-01-02], [2024-01-03]]\\noffset_array = [1979-05-27T07:32:00Z, 1979-05-27T00:32:00-07:00]\\noffset_array_positive = [1979-05-27T07:32:00+07:00]\\ndatetime_array_trailing = [1979-05-27T07:32:00Z, 1979-05-27T00:32:00-07:00,]\\noffset_fraction_dt = 1979-05-27T07:32:00.999999-07:00\\noffset_fraction_space = 1979-05-27 07:32:00.999999-07:00\\narray_offset_fraction = [1979-05-27T07:32:00.999999-07:00, 1979-05-27T07:32:00Z]\\nfraction_lower_z = 2024-01-02T03:04:05.123456z\\narray_fraction_lower_z = [2024-01-02T03:04:05.123456z]\\nutc_fraction_lower_array = [2024-01-02T03:04:05.123456z, 2024-01-02T03:04:06z]\\nutc_fraction_lower_array_trailing = [2024-01-02T03:04:05.123456z, 2024-01-02T03:04:06z,]\\nlowercase_offset_array_trailing = [2024-01-02T03:04:05z, 2024-01-02T03:04:06z,]\\nlower_hex = 0xdeadbeef\\nutc_z = 2024-01-02T03:04:05Z\\nutc_lower_z = 2024-01-02T03:04:05z\\nutc_fraction = 2024-01-02T03:04:05.123456Z\\nutc_fraction_array = [2024-01-02T03:04:05.123456Z, 2024-01-02T03:04:06Z]\\nupper_exp = 1E2\\nsigned_int_array = [+1, +0, -1]\\nspecial_float_array = [+inf, -inf, nan]\\nspecial_float_nested_arrays = [[+inf], [-inf], [nan]]\\nspecial_float_deeper_arrays = [[[+inf]], [[-inf]], [[nan]]]\\nupper_exp_nested_mixed = [[1E2, 0E0], [-1E-2]]\\nspecial_float_inline_table = {{ pos = +inf, neg = -inf, nan = nan }}\\nspecial_float_mixed_nested = [[+inf, -inf], [nan]]\\nnested_datetime_arrays = [[1979-05-27 07:32:00+07:00], [1979-05-27T00:32:00-07:00]]\\nupper_exp_nested_array = [[1E2], [+0.0E0], [-1E-2]]\\npositive_negative_offsets = [1979-05-27T07:32:00+07:00, 1979-05-27T00:32:00-07:00]\\npositive_offset_scalar_space = 1979-05-27 07:32:00+07:00\\npositive_offset_array_space = [1979-05-27 07:32:00+07:00, 1979-05-27T00:32:00-07:00]\\n'\n"
                ),
                pyo3::ffi::c_str!("test_toml_more_numeric_forms.py"),
                pyo3::ffi::c_str!("test_toml_more_numeric_forms"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
            let document = parse_template(&template).unwrap();
            let rendered = render_document(py, &document).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");

            assert_eq!(table["plus_int"].as_integer(), Some(1));
            assert_eq!(table["plus_zero"].as_integer(), Some(0));
            assert_eq!(table["plus_zero_float"].as_float(), Some(0.0));
            assert_eq!(table["zero_float_exp"].as_float(), Some(0.0));
            assert_eq!(table["plus_zero_float_exp"].as_float(), Some(0.0));
            assert_eq!(table["plus_zero_fraction_exp"].as_float(), Some(0.0));
            assert_eq!(table["exp_underscore"].as_float(), Some(1e10));
            assert_eq!(table["frac_underscore"].as_float(), Some(12.34));
            assert_eq!(
                table["local_space"]
                    .as_datetime()
                    .map(std::string::ToString::to_string),
                Some("2024-01-02T03:04:05".to_owned())
            );
            assert_eq!(
                table["local_lower_t"]
                    .as_datetime()
                    .map(std::string::ToString::to_string),
                Some("2024-01-02T03:04:05".to_owned())
            );
            assert_eq!(
                table["local_date"]
                    .as_datetime()
                    .map(std::string::ToString::to_string),
                Some("2024-01-02".to_owned())
            );
            assert_eq!(
                table["array_of_dates_trailing"]
                    .as_array()
                    .expect("array")
                    .len(),
                2
            );
            assert_eq!(
                table["mixed_date_time_array"]
                    .as_array()
                    .expect("array")
                    .len(),
                2
            );
            assert_eq!(
                table["local_time_fraction"]
                    .as_datetime()
                    .map(std::string::ToString::to_string),
                Some("03:04:05.123456".to_owned())
            );
            assert_eq!(table["array_of_dates"].as_array().expect("array").len(), 2);
            assert_eq!(
                table["array_of_local_times"]
                    .as_array()
                    .expect("array")
                    .len(),
                2
            );
            assert_eq!(
                table["nested_array_mixed_dates"]
                    .as_array()
                    .expect("array")
                    .len(),
                2
            );
            assert_eq!(table["offset_array"].as_array().expect("array").len(), 2);
            assert_eq!(
                table["offset_array_positive"]
                    .as_array()
                    .expect("array")
                    .len(),
                1
            );
            assert_eq!(
                table["datetime_array_trailing"]
                    .as_array()
                    .expect("array")
                    .len(),
                2
            );
            assert_eq!(
                table["lowercase_offset_array_trailing"]
                    .as_array()
                    .expect("array")
                    .len(),
                2
            );
            assert_eq!(
                table["offset_fraction_dt"]
                    .as_datetime()
                    .map(std::string::ToString::to_string),
                Some("1979-05-27T07:32:00.999999-07:00".to_owned())
            );
            assert_eq!(
                table["offset_fraction_space"]
                    .as_datetime()
                    .map(std::string::ToString::to_string),
                Some("1979-05-27T07:32:00.999999-07:00".to_owned())
            );
            assert_eq!(
                table["array_offset_fraction"]
                    .as_array()
                    .expect("array")
                    .len(),
                2
            );
            assert_eq!(
                table["fraction_lower_z"]
                    .as_datetime()
                    .map(std::string::ToString::to_string),
                Some("2024-01-02T03:04:05.123456Z".to_owned())
            );
            assert_eq!(
                table["array_fraction_lower_z"]
                    .as_array()
                    .expect("array")
                    .len(),
                1
            );
            assert_eq!(
                table["utc_fraction_lower_array"]
                    .as_array()
                    .expect("array")
                    .len(),
                2
            );
            assert_eq!(
                table["utc_fraction_lower_array_trailing"]
                    .as_array()
                    .expect("array")
                    .len(),
                2
            );
            assert_eq!(table["lower_hex"].as_integer(), Some(0xdead_beef));
            assert_eq!(table["upper_exp"].as_float(), Some(100.0));
            assert_eq!(
                table["signed_int_array"]
                    .as_array()
                    .expect("array")
                    .iter()
                    .filter_map(toml::Value::as_integer)
                    .collect::<Vec<_>>(),
                vec![1, 0, -1]
            );
            let special_floats = table["special_float_array"].as_array().expect("array");
            assert!(special_floats[0].as_float().expect("float").is_infinite());
            assert!(special_floats[1]
                .as_float()
                .expect("float")
                .is_sign_negative());
            assert!(special_floats[2].as_float().expect("float").is_nan());
            assert_eq!(
                table["special_float_nested_arrays"]
                    .as_array()
                    .expect("array")
                    .len(),
                3
            );
            let special_float_deeper_arrays = table["special_float_deeper_arrays"]
                .as_array()
                .expect("array");
            assert!(special_float_deeper_arrays[0][0][0]
                .as_float()
                .expect("float")
                .is_infinite());
            assert!(special_float_deeper_arrays[1][0][0]
                .as_float()
                .expect("float")
                .is_sign_negative());
            assert!(special_float_deeper_arrays[2][0][0]
                .as_float()
                .expect("float")
                .is_nan());
            assert_eq!(
                table["upper_exp_nested_mixed"]
                    .as_array()
                    .expect("array")
                    .len(),
                2
            );
            assert!(table["special_float_inline_table"]["pos"]
                .as_float()
                .expect("float")
                .is_infinite());
            assert!(table["special_float_inline_table"]["nan"]
                .as_float()
                .expect("float")
                .is_nan());
            assert_eq!(
                table["special_float_mixed_nested"]
                    .as_array()
                    .expect("array")
                    .len(),
                2
            );
            assert_eq!(
                table["nested_datetime_arrays"]
                    .as_array()
                    .expect("array")
                    .len(),
                2
            );
            assert_eq!(
                table["upper_exp_nested_array"]
                    .as_array()
                    .expect("array")
                    .len(),
                3
            );
            assert_eq!(
                table["positive_negative_offsets"]
                    .as_array()
                    .expect("array")
                    .len(),
                2
            );
            assert_eq!(
                table["positive_offset_scalar_space"]
                    .as_datetime()
                    .map(std::string::ToString::to_string),
                Some("1979-05-27T07:32:00+07:00".to_owned())
            );
            assert_eq!(
                table["positive_offset_array_space"]
                    .as_array()
                    .expect("array")
                    .len(),
                2
            );
            assert_eq!(
                table["utc_z"]
                    .as_datetime()
                    .map(std::string::ToString::to_string),
                Some("2024-01-02T03:04:05Z".to_owned())
            );
            assert_eq!(
                table["utc_lower_z"]
                    .as_datetime()
                    .map(std::string::ToString::to_string),
                Some("2024-01-02T03:04:05Z".to_owned())
            );
            assert_eq!(
                table["utc_fraction"]
                    .as_datetime()
                    .map(std::string::ToString::to_string),
                Some("2024-01-02T03:04:05.123456Z".to_owned())
            );
            assert_eq!(
                table["utc_fraction_array"].as_array().expect("array").len(),
                2
            );
        });
    }

    #[test]
    fn rejects_newlines_in_single_line_strings() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("template=t'value = \"a\\nb\"'\n"),
                pyo3::ffi::c_str!("test_toml_newline_error.py"),
                pyo3::ffi::c_str!("test_toml_newline_error"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
            let err = match parse_template(&template) {
                Ok(_) => panic!("expected TOML parse failure"),
                Err(err) => err,
            };
            assert_eq!(err.kind, ErrorKind::Parse);
            assert!(err
                .message
                .contains("single-line basic strings cannot contain newlines"));
        });
    }

    #[test]
    fn renders_toml_special_floats() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "pos=float('inf')\nneg=float('-inf')\nvalue=float('nan')\ntemplate=t'pos = {pos}\\nplus_inf = +inf\\nneg = {neg}\\nvalue = {value}\\nplus_nan = +nan\\nminus_nan = -nan\\n'\n"
                ),
                pyo3::ffi::c_str!("test_toml_special_floats.py"),
                pyo3::ffi::c_str!("test_toml_special_floats"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
            let document = parse_template(&template).unwrap();
            let rendered = render_document(py, &document).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");

            assert!(rendered.text.contains("pos = inf"));
            assert!(rendered.text.contains("plus_inf = +inf"));
            assert!(rendered.text.contains("neg = -inf"));
            assert!(rendered.text.contains("value = nan"));
            assert!(rendered.text.contains("plus_nan = +nan"));
            assert!(rendered.text.contains("minus_nan = -nan"));
            assert!(table["pos"].as_float().expect("float").is_infinite());
            assert!(table["plus_inf"].as_float().expect("float").is_infinite());
            assert!(table["neg"].as_float().expect("float").is_sign_negative());
            assert!(table["value"].as_float().expect("float").is_nan());
            assert!(table["plus_nan"].as_float().expect("float").is_nan());
            assert!(table["minus_nan"].as_float().expect("float").is_nan());
        });
    }

    #[test]
    fn parses_arrays_with_trailing_commas() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ntemplate=Template('value = [1, 2,]\\nnested = [[ ], [1, 2,],]\\nempty_inline_tables = [{}, {}]\\nnested_empty_inline_arrays = { inner = [[], [1]] }\\n')\n"
                ),
                pyo3::ffi::c_str!("test_toml_trailing_comma.py"),
                pyo3::ffi::c_str!("test_toml_trailing_comma"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
            let document = parse_template(&template).unwrap();
            let rendered = render_document(py, &document).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");

            assert_eq!(
                table["value"]
                    .as_array()
                    .expect("array")
                    .iter()
                    .filter_map(toml::Value::as_integer)
                    .collect::<Vec<_>>(),
                vec![1, 2]
            );
            assert_eq!(table["nested"].as_array().expect("array").len(), 2);
            assert_eq!(
                table["empty_inline_tables"]
                    .as_array()
                    .expect("array")
                    .len(),
                2
            );
            assert_eq!(
                table["nested_empty_inline_arrays"]["inner"]
                    .as_array()
                    .expect("array")
                    .len(),
                2
            );
        });
    }

    #[test]
    fn renders_nested_collections_and_array_tables() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ntemplate=Template('matrix = [[1, 2], [3, 4]]\\nmeta = { inner = { value = 1 } }\\nnested_inline_arrays = { items = [[1, 2], [3, 4]] }\\ndeep_nested_inline = { inner = { deep = { value = 1 } } }\\ninline_table_array = [{ a = 1 }, { a = 2 }]\\ninline_table_array_nested = [[{ a = 1 }], [{ a = 2 }]]\\n[a]\\nvalue = 1\\n[[a.b]]\\nname = \"x\"\\n[[services]]\\nname = \"api\"\\n[[services]]\\nname = \"worker\"\\n')\n"
                ),
                pyo3::ffi::c_str!("test_toml_nested_collections.py"),
                pyo3::ffi::c_str!("test_toml_nested_collections"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
            let document = parse_template(&template).unwrap();
            let rendered = render_document(py, &document).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");

            assert_eq!(table["matrix"].as_array().expect("array").len(), 2);
            assert_eq!(table["meta"]["inner"]["value"].as_integer(), Some(1));
            assert_eq!(
                table["nested_inline_arrays"]["items"]
                    .as_array()
                    .expect("array")
                    .len(),
                2
            );
            assert_eq!(
                table["deep_nested_inline"]["inner"]["deep"]["value"].as_integer(),
                Some(1)
            );
            assert_eq!(
                table["inline_table_array"].as_array().expect("array").len(),
                2
            );
            assert_eq!(table["inline_table_array"][0]["a"].as_integer(), Some(1));
            assert_eq!(table["inline_table_array"][1]["a"].as_integer(), Some(2));
            assert_eq!(
                table["inline_table_array_nested"]
                    .as_array()
                    .expect("array")
                    .len(),
                2
            );
            assert_eq!(
                table["inline_table_array_nested"][0]
                    .as_array()
                    .expect("array")[0]["a"]
                    .as_integer(),
                Some(1)
            );
            assert_eq!(
                table["inline_table_array_nested"][1]
                    .as_array()
                    .expect("array")[0]["a"]
                    .as_integer(),
                Some(2)
            );
            assert_eq!(table["a"]["value"].as_integer(), Some(1));
            assert_eq!(table["a"]["b"].as_array().expect("array").len(), 1);
            assert_eq!(table["a"]["b"][0]["name"].as_str(), Some("x"));
            assert_eq!(table["services"].as_array().expect("array").len(), 2);
        });
    }

    #[test]
    fn parses_headers_comments_and_crlf_literal_strings() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nquoted_header=t'[\"a.b\"]\\nvalue = 1\\n'\ndotted_header=t'[site.\"google.com\"]\\nvalue = 1\\n'\nquoted_segments=t'[\"a\".\"b\"]\\nvalue = 1\\n'\nquoted_header_then_dotted=Template('[\"a.b\"]\\nvalue = 1\\n\\n[\"a.b\".c]\\nname = \"x\"\\n')\ninline_comment=Template('value = { a = 1 } # comment\\n')\ncommented_array=t'value = [\\n  1,\\n  # comment\\n  2,\\n]\\n'\nliteral_crlf=Template(\"value = '''a\\r\\nb'''\\n\")\narray_then_table=t'[[items]]\\nname = \"a\"\\n\\n[tool]\\nvalue = 1\\n'\n"
                ),
                pyo3::ffi::c_str!("test_toml_additional_surface.py"),
                pyo3::ffi::c_str!("test_toml_additional_surface"),
            )
            .unwrap();

            let quoted_header = module.getattr("quoted_header").unwrap();
            let quoted_header = extract_template(py, &quoted_header, "toml_t/toml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&quoted_header).unwrap()).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");
            assert_eq!(table["a.b"]["value"].as_integer(), Some(1));

            let dotted_header = module.getattr("dotted_header").unwrap();
            let dotted_header = extract_template(py, &dotted_header, "toml_t/toml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&dotted_header).unwrap()).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");
            assert_eq!(table["site"]["google.com"]["value"].as_integer(), Some(1));

            let quoted_segments = module.getattr("quoted_segments").unwrap();
            let quoted_segments =
                extract_template(py, &quoted_segments, "toml_t/toml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&quoted_segments).unwrap()).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");
            assert_eq!(table["a"]["b"]["value"].as_integer(), Some(1));

            let quoted_header_then_dotted = module.getattr("quoted_header_then_dotted").unwrap();
            let quoted_header_then_dotted =
                extract_template(py, &quoted_header_then_dotted, "toml_t/toml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&quoted_header_then_dotted).unwrap()).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");
            assert_eq!(table["a.b"]["value"].as_integer(), Some(1));
            assert_eq!(table["a.b"]["c"]["name"].as_str(), Some("x"));

            let inline_comment = module.getattr("inline_comment").unwrap();
            let inline_comment =
                extract_template(py, &inline_comment, "toml_t/toml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&inline_comment).unwrap()).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");
            assert_eq!(table["value"]["a"].as_integer(), Some(1));

            let commented_array = module.getattr("commented_array").unwrap();
            let commented_array =
                extract_template(py, &commented_array, "toml_t/toml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&commented_array).unwrap()).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");
            assert_eq!(
                table["value"]
                    .as_array()
                    .expect("array")
                    .iter()
                    .filter_map(toml::Value::as_integer)
                    .collect::<Vec<_>>(),
                vec![1, 2]
            );

            let literal_crlf = module.getattr("literal_crlf").unwrap();
            let literal_crlf = extract_template(py, &literal_crlf, "toml_t/toml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&literal_crlf).unwrap()).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");
            assert_eq!(table["value"].as_str(), Some("a\nb"));

            let array_then_table = module.getattr("array_then_table").unwrap();
            let array_then_table =
                extract_template(py, &array_then_table, "toml_t/toml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&array_then_table).unwrap()).unwrap();
            let table = rendered.data.as_table().expect("expected TOML table");
            assert_eq!(table["items"].as_array().expect("array").len(), 1);
            assert_eq!(table["tool"]["value"].as_integer(), Some(1));
        });
    }

    #[test]
    fn rejects_multiline_inline_tables() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ntemplate=Template('value = { a = 1,\\n b = 2 }\\n')\n"
                ),
                pyo3::ffi::c_str!("test_toml_inline_table_newline.py"),
                pyo3::ffi::c_str!("test_toml_inline_table_newline"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
            let err = match parse_template(&template) {
                Ok(_) => panic!("expected TOML parse failure"),
                Err(err) => err,
            };
            assert_eq!(err.kind, ErrorKind::Parse);
            assert!(err.message.contains("Expected a TOML key segment"));
        });
    }

    #[test]
    fn rejects_invalid_table_redefinitions_and_newlines_after_dots() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ntable_redefine=Template('[a]\\nvalue = 1\\n[a]\\nname = \"x\"\\n')\narray_redefine=Template('[[a]]\\nname = \"x\"\\n[a]\\nvalue = 1\\n')\nnewline_after_dot=Template('a.\\nb = 1\\n')\nextra_array_comma=Template('value = [1,,2]\\n')\ninline_double_comma=Template('value = { a = 1,, b = 2 }\\n')\narray_leading_comma=Template('value = [,1]\\n')\ninline_trailing_comma=Template('value = { a = 1, }\\n')\ninvalid_decimal_underscore=Template('value = 1__2\\n')\ninvalid_hex_underscore=Template('value = 0x_DEAD\\n')\nheader_trailing_dot=Template('[a.]\\nvalue = 1\\n')\ninvalid_octal_underscore=Template('value = 0o_7\\n')\ninvalid_fraction_underscore=Template('value = 1_.0\\n')\ninvalid_plus_zero_float_underscore=Template('value = +0_.0\\n')\ninvalid_fraction_double_underscore=Template('value = 1_2.3__4\\n')\ninvalid_exp_double_underscore=Template('value = 1e1__0\\n')\ninvalid_double_plus=Template('value = ++1\\n')\ninvalid_double_plus_inline_table=Template('value = { pos = ++1 }\\n')\ninvalid_double_plus_nested=Template('value = { inner = { deeper = ++1 } }\\n')\ninvalid_double_plus_nested_inline_table=Template('value = { inner = { pos = ++1 } }\\n')\ninvalid_double_plus_array_nested=Template('value = [[++1]]\\n')\ninvalid_double_plus_array_mixed=Template('value = [1, ++1]\\n')\ninvalid_double_plus_after_scalar=Template('value = [1, 2, ++1]\\n')\nleading_zero=Template('value = 00\\n')\nleading_zero_plus=Template('value = +01\\n')\nleading_zero_float=Template('value = 01.2\\n')\nbinary_leading_underscore=Template('value = 0b_1\\n')\nsigned_binary=Template('value = +0b1\\n')\ntime_with_offset=Template('value = 03:04:05+09:00\\n')\nplus_inf_underscore=Template('value = +inf_\\n')\nplus_nan_underscore=Template('value = +nan_\\n')\ntime_lower_z=Template('value = 03:04:05z\\n')\ninvalid_exp_leading_underscore=Template('value = 1e_1\\n')\ninvalid_exp_trailing_underscore=Template('value = 1e1_\\n')\ndouble_sign_exp=Template('value = 1e--1\\n')\ndouble_sign_float=Template('value = --1.0\\n')\ndouble_dot_dotted_key=Template('a..b = 1\\n')\nhex_float_like=Template('value = 0x1.2\\n')\nsigned_octal=Template('value = -0o7\\n')\ninline_table_missing_comma=Template('value = { a = 1 b = 2 }\\n')\n"
                ),
                pyo3::ffi::c_str!("test_toml_invalid_tables.py"),
                pyo3::ffi::c_str!("test_toml_invalid_tables"),
            )
            .unwrap();

            for name in ["table_redefine", "array_redefine"] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
                let document = parse_template(&template).unwrap();
                let rendered = render_document(py, &document).unwrap();
                let err =
                    parse_rendered_toml(&rendered.text).expect_err("expected TOML parse failure");
                assert_eq!(err.kind, ErrorKind::Parse);
                assert!(
                    err.message.contains("duplicate key"),
                    "{name}: {}",
                    err.message
                );
            }

            let template = module.getattr("newline_after_dot").unwrap();
            let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
            let err = match parse_template(&template) {
                Ok(_) => panic!("expected TOML parse failure for newline_after_dot"),
                Err(err) => err,
            };
            assert_eq!(err.kind, ErrorKind::Parse);
            assert!(err.message.contains("Expected a TOML key segment"));

            for (name, expected) in [
                ("extra_array_comma", "Expected a TOML value"),
                ("inline_double_comma", "Expected a TOML key segment"),
                ("array_leading_comma", "Expected a TOML value"),
                ("inline_trailing_comma", "Expected a TOML key segment"),
                ("invalid_decimal_underscore", "Invalid TOML literal"),
                ("invalid_hex_underscore", "Invalid TOML literal"),
                ("header_trailing_dot", "Expected a TOML key segment"),
                ("invalid_octal_underscore", "Invalid TOML literal"),
                ("invalid_fraction_underscore", "Invalid TOML literal"),
                ("invalid_plus_zero_float_underscore", "Invalid TOML literal"),
                ("invalid_fraction_double_underscore", "Invalid TOML literal"),
                ("invalid_exp_double_underscore", "Invalid TOML literal"),
                ("invalid_double_plus", "Invalid TOML literal"),
                ("invalid_double_plus_inline_table", "Invalid TOML literal"),
                ("invalid_double_plus_nested", "Invalid TOML literal"),
                (
                    "invalid_double_plus_nested_inline_table",
                    "Invalid TOML literal",
                ),
                ("invalid_double_plus_array_nested", "Invalid TOML literal"),
                ("invalid_double_plus_array_mixed", "Invalid TOML literal"),
                ("invalid_double_plus_after_scalar", "Invalid TOML literal"),
                ("leading_zero", "Invalid TOML literal"),
                ("leading_zero_plus", "Invalid TOML literal"),
                ("leading_zero_float", "Invalid TOML literal"),
                ("binary_leading_underscore", "Invalid TOML literal"),
                ("signed_binary", "Invalid TOML literal"),
                ("time_with_offset", "Invalid TOML literal"),
                ("plus_inf_underscore", "Invalid TOML literal"),
                ("plus_nan_underscore", "Invalid TOML literal"),
                ("time_lower_z", "Invalid TOML literal"),
                ("invalid_exp_leading_underscore", "Invalid TOML literal"),
                ("invalid_exp_trailing_underscore", "Invalid TOML literal"),
                ("double_sign_exp", "Invalid TOML literal"),
                ("double_sign_float", "Invalid TOML literal"),
                ("double_dot_dotted_key", "Expected a TOML key segment"),
                ("hex_float_like", "Invalid TOML literal"),
                ("signed_octal", "Invalid TOML literal"),
                ("inline_table_missing_comma", "Invalid TOML literal"),
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
                let err = parse_template(&template).expect_err("expected TOML parse failure");
                assert_eq!(err.kind, ErrorKind::Parse);
                assert!(err.message.contains(expected), "{name}: {}", err.message);
            }
        });
    }

    #[test]
    fn rejects_value_contracts() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from datetime import UTC, time\nfrom string.templatelib import Template\nclass BadStringValue:\n    def __str__(self):\n        raise ValueError('cannot stringify')\nbad_key=3\nbad_time=time(1, 2, 3, tzinfo=UTC)\nbad_fragment=BadStringValue()\nkey_template=t'{bad_key} = 1'\nnull_template=t'name = {None}'\ntime_template=t'when = {bad_time}'\nfragment_template=t'title = \"hi-{bad_fragment}\"'\nduplicate_table=Template('[a]\\nvalue = 1\\n[a]\\nname = \"x\"\\n')\n"
                ),
                pyo3::ffi::c_str!("test_toml_value_contracts.py"),
                pyo3::ffi::c_str!("test_toml_value_contracts"),
            )
            .unwrap();

            for (name, expected) in [
                ("key_template", "TOML keys must be str"),
                ("null_template", "TOML has no null value"),
                ("time_template", "timezone"),
                ("fragment_template", "string fragment"),
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
                let document = parse_template(&template).unwrap();
                let err = match render_document(py, &document) {
                    Ok(_) => panic!("expected TOML render failure"),
                    Err(err) => err,
                };
                assert_eq!(err.kind, ErrorKind::Unrepresentable);
                assert!(err.message.contains(expected), "{name}: {}", err.message);
            }

            let duplicate_table = module.getattr("duplicate_table").unwrap();
            let duplicate_table =
                extract_template(py, &duplicate_table, "toml_t/toml_t_str").unwrap();
            let document = parse_template(&duplicate_table).unwrap();
            let rendered = render_document(py, &document).unwrap();
            let err = parse_rendered_toml(&rendered.text)
                .expect_err("expected TOML duplicate-key parse failure");
            assert_eq!(err.kind, ErrorKind::Parse);
            assert!(err.message.contains("duplicate key"));
        });
    }

    #[test]
    fn rejects_invalid_numeric_literal_families() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nleading_zero=Template('value = 00\\n')\npositive_leading_zero=Template('value = +01\\n')\ndouble_underscore=Template('value = 1__2\\n')\nhex_underscore=Template('value = 0x_DEAD\\n')\ndouble_plus=Template('value = ++1\\n')\n"
                ),
                pyo3::ffi::c_str!("test_toml_invalid_literals.py"),
                pyo3::ffi::c_str!("test_toml_invalid_literals"),
            )
            .unwrap();

            for name in [
                "leading_zero",
                "positive_leading_zero",
                "double_underscore",
                "hex_underscore",
                "double_plus",
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
                let err = parse_template(&template).expect_err("expected TOML parse failure");
                assert_eq!(err.kind, ErrorKind::Parse);
                assert!(
                    err.message.contains("Invalid TOML literal"),
                    "{name}: {}",
                    err.message
                );
            }
        });
    }

    #[test]
    fn rejects_additional_invalid_literal_families() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ninvalid_exp_mixed_sign=Template('value = 1e_+1\\n')\ninvalid_float_then_exp=Template('value = 1.e1\\n')\ninvalid_inline_pos=Template('value = { pos = ++1 }\\n')\ninvalid_nested_inline_pos=Template('value = { inner = { pos = ++1 } }\\n')\ninvalid_triple_nested_plus=Template('value = [[[++1]]]\\n')\n"
                ),
                pyo3::ffi::c_str!("test_toml_additional_invalid_literals.py"),
                pyo3::ffi::c_str!("test_toml_additional_invalid_literals"),
            )
            .unwrap();

            for name in [
                "invalid_exp_mixed_sign",
                "invalid_float_then_exp",
                "invalid_inline_pos",
                "invalid_nested_inline_pos",
                "invalid_triple_nested_plus",
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
                let err = parse_template(&template).expect_err("expected TOML parse failure");
                assert_eq!(err.kind, ErrorKind::Parse);
                assert!(
                    err.message.contains("Invalid TOML literal"),
                    "{name}: {}",
                    err.message
                );
            }
        });
    }

    #[test]
    fn rejects_bare_literal_fragment_and_suffix_families() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "count=1\nfragment=t'value = 2{count}\\n'\nsuffix=t'value = {count}ms\\n'\n"
                ),
                pyo3::ffi::c_str!("test_toml_literal_fragments.py"),
                pyo3::ffi::c_str!("test_toml_literal_fragments"),
            )
            .unwrap();

            for (name, expected) in [
                (
                    "fragment",
                    "TOML bare literals cannot contain fragment interpolations.",
                ),
                (
                    "suffix",
                    "Whole-value TOML interpolations cannot have bare suffix text.",
                ),
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
                let err = parse_template(&template).expect_err("expected TOML parse failure");
                assert_eq!(err.kind, ErrorKind::Parse);
                assert!(err.message.contains(expected), "{name}: {}", err.message);
            }
        });
    }

    #[test]
    fn renders_header_progressions_comments_and_crlf_text() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nquoted_header_then_dotted=Template('[\"a.b\"]\\nvalue = 1\\n\\n[\"a.b\".c]\\nname = \"x\"\\n')\ncommented_array=t'value = [\\n  1,\\n  # comment\\n  2,\\n]\\n'\nliteral_crlf=Template(\"value = '''a\\r\\nb'''\\n\")\narray_then_table=t'[[items]]\\nname = \"a\"\\n\\n[tool]\\nvalue = 1\\n'\n"
                ),
                pyo3::ffi::c_str!("test_toml_render_progressions.py"),
                pyo3::ffi::c_str!("test_toml_render_progressions"),
            )
            .unwrap();

            let quoted_header_then_dotted = module.getattr("quoted_header_then_dotted").unwrap();
            let quoted_header_then_dotted =
                extract_template(py, &quoted_header_then_dotted, "toml_t/toml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&quoted_header_then_dotted).unwrap()).unwrap();
            assert_eq!(
                rendered.text,
                "[\"a.b\"]\nvalue = 1\n[\"a.b\".c]\nname = \"x\""
            );
            let table = rendered.data.as_table().expect("expected TOML table");
            assert_eq!(table["a.b"]["c"]["name"].as_str(), Some("x"));

            let commented_array = module.getattr("commented_array").unwrap();
            let commented_array =
                extract_template(py, &commented_array, "toml_t/toml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&commented_array).unwrap()).unwrap();
            assert_eq!(rendered.text, "value = [1, 2]");
            assert_eq!(
                rendered.data["value"]
                    .as_array()
                    .expect("array")
                    .iter()
                    .filter_map(toml::Value::as_integer)
                    .collect::<Vec<_>>(),
                vec![1, 2]
            );

            let literal_crlf = module.getattr("literal_crlf").unwrap();
            let literal_crlf = extract_template(py, &literal_crlf, "toml_t/toml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&literal_crlf).unwrap()).unwrap();
            assert_eq!(rendered.text, "value = \"a\\nb\"");
            assert_eq!(rendered.data["value"].as_str(), Some("a\nb"));

            let array_then_table = module.getattr("array_then_table").unwrap();
            let array_then_table =
                extract_template(py, &array_then_table, "toml_t/toml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&array_then_table).unwrap()).unwrap();
            assert_eq!(rendered.text, "[[items]]\nname = \"a\"\n[tool]\nvalue = 1");
            let table = rendered.data.as_table().expect("expected TOML table");
            assert_eq!(table["items"].as_array().expect("array").len(), 1);
            assert_eq!(table["tool"]["value"].as_integer(), Some(1));
        });
    }

    #[test]
    fn renders_temporal_values_and_special_float_arrays() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from datetime import date, datetime, time, timedelta, timezone\nlocal_date=date(2024, 1, 2)\nlocal_time=time(3, 4, 5, 678901)\noffset_time=datetime(1979, 5, 27, 7, 32, 0, 999999, tzinfo=timezone(timedelta(hours=-7)))\ndoc=t'local_date = {local_date}\\nlocal_time = {local_time}\\noffset_times = [{offset_time}]\\nspecial = [+inf, -inf, nan]\\n'\n"
                ),
                pyo3::ffi::c_str!("test_toml_temporal_arrays.py"),
                pyo3::ffi::c_str!("test_toml_temporal_arrays"),
            )
            .unwrap();

            let doc = module.getattr("doc").unwrap();
            let doc = extract_template(py, &doc, "toml_t/toml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&doc).unwrap()).unwrap();
            assert_eq!(
                rendered.text,
                "local_date = 2024-01-02\nlocal_time = 03:04:05.678901\noffset_times = [1979-05-27T07:32:00.999999-07:00]\nspecial = [+inf, -inf, nan]"
            );
            assert_eq!(
                rendered.data["local_date"]
                    .as_datetime()
                    .map(ToString::to_string),
                Some("2024-01-02".to_string())
            );
            assert_eq!(
                rendered.data["local_time"]
                    .as_datetime()
                    .map(ToString::to_string),
                Some("03:04:05.678901".to_string())
            );
            assert_eq!(
                rendered.data["offset_times"][0]
                    .as_datetime()
                    .map(ToString::to_string),
                Some("1979-05-27T07:32:00.999999-07:00".to_string())
            );
            assert_eq!(
                rendered.data["special"]
                    .as_array()
                    .expect("special array")
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
                vec!["inf".to_string(), "-inf".to_string(), "nan".to_string()]
            );
        });
    }

    #[test]
    fn renders_end_to_end_supported_positions_text_and_data() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from datetime import UTC, datetime\nkey='leaf'\nleft='prefix'\nright='suffix'\ncreated=datetime(2024, 1, 2, 3, 4, 5, tzinfo=UTC)\ntemplate=t'''\ntitle = \"item-{left}\"\n[root.{key}]\nname = {right}\nlabel = \"{left}-{right}\"\ncreated = {created}\nrows = [{left}, {right}]\nmeta = {{ enabled = true, target = {right} }}\n'''\n"
                ),
                pyo3::ffi::c_str!("test_toml_end_to_end_positions.py"),
                pyo3::ffi::c_str!("test_toml_end_to_end_positions"),
            )
            .unwrap();

            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
            assert_eq!(
                rendered.text,
                "title = \"item-prefix\"\n[root.\"leaf\"]\nname = \"suffix\"\nlabel = \"prefix-suffix\"\ncreated = 2024-01-02T03:04:05+00:00\nrows = [\"prefix\", \"suffix\"]\nmeta = { enabled = true, target = \"suffix\" }"
            );
            let table = rendered.data.as_table().expect("expected TOML table");
            assert_eq!(table["title"].as_str(), Some("item-prefix"));
            assert_eq!(table["root"]["leaf"]["name"].as_str(), Some("suffix"));
            assert_eq!(
                table["root"]["leaf"]["label"].as_str(),
                Some("prefix-suffix")
            );
            assert_eq!(
                table["root"]["leaf"]["created"]
                    .as_datetime()
                    .map(ToString::to_string),
                Some("2024-01-02T03:04:05+00:00".to_string())
            );
            assert_eq!(
                table["root"]["leaf"]["rows"]
                    .as_array()
                    .expect("rows")
                    .iter()
                    .filter_map(toml::Value::as_str)
                    .collect::<Vec<_>>(),
                vec!["prefix", "suffix"]
            );
            assert_eq!(
                table["root"]["leaf"]["meta"]["target"].as_str(),
                Some("suffix")
            );
        });
    }

    #[test]
    fn renders_string_families_exact_text_and_data() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "value='name'\nbasic=t'basic = \"hi-{value}\"'\nliteral=t\"literal = 'hi-{value}'\"\nmulti_basic=t'multi_basic = \"\"\"hi-{value}\"\"\"'\nmulti_literal=t\"\"\"multi_literal = '''hi-{value}'''\"\"\"\n"
                ),
                pyo3::ffi::c_str!("test_toml_string_families_render.py"),
                pyo3::ffi::c_str!("test_toml_string_families_render"),
            )
            .unwrap();

            for (name, expected_key) in [
                ("basic", "basic"),
                ("literal", "literal"),
                ("multi_basic", "multi_basic"),
                ("multi_literal", "multi_literal"),
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
                let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
                assert_eq!(rendered.data[expected_key].as_str(), Some("hi-name"));
                assert!(
                    rendered.text.contains("hi-name"),
                    "{name}: {}",
                    rendered.text
                );
            }
        });
    }

    #[test]
    fn renders_date_and_time_round_trip_shapes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from datetime import date, time\nday=date(2024, 1, 2)\nmoment=time(4, 5, 6)\ntemplate=t'day = {day}\\nmoment = {moment}'\n"
                ),
                pyo3::ffi::c_str!("test_toml_date_time_round_trip.py"),
                pyo3::ffi::c_str!("test_toml_date_time_round_trip"),
            )
            .unwrap();

            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
            assert_eq!(rendered.text, "day = 2024-01-02\nmoment = 04:05:06");
            assert_eq!(
                rendered.data["day"].as_datetime().map(ToString::to_string),
                Some("2024-01-02".to_string())
            );
            assert_eq!(
                rendered.data["moment"]
                    .as_datetime()
                    .map(ToString::to_string),
                Some("04:05:06".to_string())
            );
        });
    }

    #[test]
    fn renders_array_tables_and_comment_preserving_shapes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "name='api'\nworker='worker'\ntemplate=t'''\n# comment before content\n[[services]]\nname = {name} # inline comment\n\n[[services]]\nname = {worker}\n'''\n"
                ),
                pyo3::ffi::c_str!("test_toml_array_tables_comments.py"),
                pyo3::ffi::c_str!("test_toml_array_tables_comments"),
            )
            .unwrap();

            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
            assert_eq!(
                rendered.text,
                "[[services]]\nname = \"api\"\n[[services]]\nname = \"worker\""
            );
            let services = rendered.data["services"]
                .as_array()
                .expect("services array");
            assert_eq!(services.len(), 2);
            assert_eq!(services[0]["name"].as_str(), Some("api"));
            assert_eq!(services[1]["name"].as_str(), Some("worker"));
        });
    }

    #[test]
    fn renders_array_of_tables_spec_example_text_and_data() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "template=t'[[products]]\\nname = \"Hammer\"\\nsku = 738594937\\n\\n[[products]]\\nname = \"Nail\"\\nsku = 284758393\\ncolor = \"gray\"\\n'\n"
                ),
                pyo3::ffi::c_str!("test_toml_array_of_tables_spec_example.py"),
                pyo3::ffi::c_str!("test_toml_array_of_tables_spec_example"),
            )
            .unwrap();

            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
            assert_eq!(
                rendered.text,
                "[[products]]\nname = \"Hammer\"\nsku = 738594937\n[[products]]\nname = \"Nail\"\nsku = 284758393\ncolor = \"gray\""
            );
            let products = rendered.data["products"]
                .as_array()
                .expect("products array");
            assert_eq!(products.len(), 2);
            assert_eq!(products[0]["name"].as_str(), Some("Hammer"));
            assert_eq!(products[0]["sku"].as_integer(), Some(738594937));
            assert_eq!(products[1]["name"].as_str(), Some("Nail"));
            assert_eq!(products[1]["sku"].as_integer(), Some(284758393));
            assert_eq!(products[1]["color"].as_str(), Some("gray"));
        });
    }

    #[test]
    fn renders_nested_array_of_tables_spec_hierarchy_text_and_data() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "template=t'[[fruit]]\\nname = \"apple\"\\n\\n[fruit.physical]\\ncolor = \"red\"\\nshape = \"round\"\\n\\n[[fruit.variety]]\\nname = \"red delicious\"\\n\\n[[fruit.variety]]\\nname = \"granny smith\"\\n\\n[[fruit]]\\nname = \"banana\"\\n\\n[[fruit.variety]]\\nname = \"plantain\"\\n'\n"
                ),
                pyo3::ffi::c_str!("test_toml_nested_array_tables_spec_example.py"),
                pyo3::ffi::c_str!("test_toml_nested_array_tables_spec_example"),
            )
            .unwrap();

            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
            assert_eq!(
                rendered.text,
                "[[fruit]]\nname = \"apple\"\n[fruit.physical]\ncolor = \"red\"\nshape = \"round\"\n[[fruit.variety]]\nname = \"red delicious\"\n[[fruit.variety]]\nname = \"granny smith\"\n[[fruit]]\nname = \"banana\"\n[[fruit.variety]]\nname = \"plantain\""
            );
            let fruit = rendered.data["fruit"].as_array().expect("fruit array");
            assert_eq!(fruit.len(), 2);
            assert_eq!(fruit[0]["name"].as_str(), Some("apple"));
            assert_eq!(fruit[0]["physical"]["color"].as_str(), Some("red"));
            assert_eq!(fruit[0]["physical"]["shape"].as_str(), Some("round"));
            let varieties = fruit[0]["variety"].as_array().expect("apple varieties");
            assert_eq!(varieties.len(), 2);
            assert_eq!(varieties[0]["name"].as_str(), Some("red delicious"));
            assert_eq!(varieties[1]["name"].as_str(), Some("granny smith"));
            assert_eq!(fruit[1]["name"].as_str(), Some("banana"));
            let varieties = fruit[1]["variety"].as_array().expect("banana varieties");
            assert_eq!(varieties.len(), 1);
            assert_eq!(varieties[0]["name"].as_str(), Some("plantain"));
        });
    }

    #[test]
    fn renders_main_spec_example_text_and_data() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "template=t'title = \"TOML Example\"\\n\\n[owner]\\nname = \"Tom Preston-Werner\"\\ndob = 1979-05-27T07:32:00-08:00\\n\\n[database]\\nenabled = true\\nports = [ 8000, 8001, 8002 ]\\ndata = [ [\"delta\", \"phi\"], [3.14] ]\\ntemp_targets = {{ cpu = 79.5, case = 72.0 }}\\n\\n[servers]\\n\\n[servers.alpha]\\nip = \"10.0.0.1\"\\nrole = \"frontend\"\\n\\n[servers.beta]\\nip = \"10.0.0.2\"\\nrole = \"backend\"\\n'\n"
                ),
                pyo3::ffi::c_str!("test_toml_main_spec_example.py"),
                pyo3::ffi::c_str!("test_toml_main_spec_example"),
            )
            .unwrap();

            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
            assert_eq!(
                rendered.text,
                "title = \"TOML Example\"\n[owner]\nname = \"Tom Preston-Werner\"\ndob = 1979-05-27T07:32:00-08:00\n[database]\nenabled = true\nports = [8000, 8001, 8002]\ndata = [[\"delta\", \"phi\"], [3.14]]\ntemp_targets = { cpu = 79.5, case = 72.0 }\n[servers]\n[servers.alpha]\nip = \"10.0.0.1\"\nrole = \"frontend\"\n[servers.beta]\nip = \"10.0.0.2\"\nrole = \"backend\""
            );
            assert_eq!(rendered.data["title"].as_str(), Some("TOML Example"));
            assert_eq!(
                rendered.data["owner"]["name"].as_str(),
                Some("Tom Preston-Werner")
            );
            assert_eq!(rendered.data["database"]["enabled"].as_bool(), Some(true));
            assert_eq!(
                rendered.data["database"]["ports"]
                    .as_array()
                    .expect("ports array")
                    .len(),
                3
            );
            assert_eq!(
                rendered.data["servers"]["alpha"]["ip"].as_str(),
                Some("10.0.0.1")
            );
            assert_eq!(
                rendered.data["servers"]["beta"]["role"].as_str(),
                Some("backend")
            );
        });
    }

    #[test]
    fn renders_empty_headers_and_empty_path_segments() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "empty_header=t'[\"\"]\\nvalue = 1\\n'\nempty_segment=t'a.\"\".b = 1\\n'\nempty_subsegments=t'[\"\".\"\".leaf]\\nvalue = 1\\n'\n"
                ),
                pyo3::ffi::c_str!("test_toml_empty_path_shapes.py"),
                pyo3::ffi::c_str!("test_toml_empty_path_shapes"),
            )
            .unwrap();

            let empty_header = module.getattr("empty_header").unwrap();
            let empty_header = extract_template(py, &empty_header, "toml_t/toml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&empty_header).unwrap()).unwrap();
            assert_eq!(rendered.text, "[\"\"]\nvalue = 1");
            assert_eq!(rendered.data[""]["value"].as_integer(), Some(1));

            let empty_segment = module.getattr("empty_segment").unwrap();
            let empty_segment = extract_template(py, &empty_segment, "toml_t/toml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&empty_segment).unwrap()).unwrap();
            assert_eq!(rendered.text, "a.\"\".b = 1");
            assert_eq!(rendered.data["a"][""]["b"].as_integer(), Some(1));

            let empty_subsegments = module.getattr("empty_subsegments").unwrap();
            let empty_subsegments =
                extract_template(py, &empty_subsegments, "toml_t/toml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&empty_subsegments).unwrap()).unwrap();
            assert_eq!(rendered.text, "[\"\".\"\".leaf]\nvalue = 1");
            assert_eq!(rendered.data[""][""]["leaf"]["value"].as_integer(), Some(1));
        });
    }

    #[test]
    fn renders_special_float_nested_shapes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "template=t'special_float_inline_table = {{ pos = +inf, neg = -inf, nan = nan }}\\nspecial_float_mixed_nested = [[+inf, -inf], [nan]]\\n'\n"
                ),
                pyo3::ffi::c_str!("test_toml_special_float_nested.py"),
                pyo3::ffi::c_str!("test_toml_special_float_nested"),
            )
            .unwrap();

            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
            assert_eq!(
                rendered.text,
                "special_float_inline_table = { pos = +inf, neg = -inf, nan = nan }\nspecial_float_mixed_nested = [[+inf, -inf], [nan]]"
            );
            assert!(rendered.data["special_float_inline_table"]["pos"]
                .as_float()
                .expect("pos float")
                .is_infinite());
            assert!(rendered.data["special_float_inline_table"]["neg"]
                .as_float()
                .expect("neg float")
                .is_sign_negative());
            assert!(rendered.data["special_float_inline_table"]["nan"]
                .as_float()
                .expect("nan float")
                .is_nan());
            assert!(rendered.data["special_float_mixed_nested"][0][0]
                .as_float()
                .expect("nested pos")
                .is_infinite());
            assert!(rendered.data["special_float_mixed_nested"][0][1]
                .as_float()
                .expect("nested neg")
                .is_sign_negative());
            assert!(rendered.data["special_float_mixed_nested"][1][0]
                .as_float()
                .expect("nested nan")
                .is_nan());
        });
    }

    #[test]
    fn renders_numeric_and_datetime_literal_shapes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ntemplate=Template('plus_int = +1\\nplus_zero = +0\\nplus_zero_float = +0.0\\nlocal_date = 2024-01-02\\nlocal_time_fraction = 03:04:05.123456\\noffset_fraction_dt = 1979-05-27T07:32:00.999999-07:00\\nutc_fraction_lower_array = [2024-01-02T03:04:05.123456z, 2024-01-02T03:04:06z]\\nsigned_int_array = [+1, +0, -1]\\n')\n"
                ),
                pyo3::ffi::c_str!("test_toml_numeric_datetime_literal_shapes.py"),
                pyo3::ffi::c_str!("test_toml_numeric_datetime_literal_shapes"),
            )
            .unwrap();

            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "toml_t/toml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
            assert_eq!(
                rendered.text,
                "plus_int = +1\nplus_zero = +0\nplus_zero_float = +0.0\nlocal_date = 2024-01-02\nlocal_time_fraction = 03:04:05.123456\noffset_fraction_dt = 1979-05-27T07:32:00.999999-07:00\nutc_fraction_lower_array = [2024-01-02T03:04:05.123456z, 2024-01-02T03:04:06z]\nsigned_int_array = [+1, +0, -1]"
            );
            assert_eq!(rendered.data["plus_int"].as_integer(), Some(1));
            assert_eq!(rendered.data["plus_zero"].as_integer(), Some(0));
            assert_eq!(rendered.data["plus_zero_float"].as_float(), Some(0.0));
            assert_eq!(
                rendered.data["local_date"]
                    .as_datetime()
                    .and_then(|value| value.date.as_ref())
                    .map(ToString::to_string),
                Some("2024-01-02".to_string())
            );
            assert_eq!(
                rendered.data["local_time_fraction"]
                    .as_datetime()
                    .and_then(|value| value.time.as_ref())
                    .map(ToString::to_string),
                Some("03:04:05.123456".to_string())
            );
            assert_eq!(
                rendered.data["offset_fraction_dt"]
                    .as_datetime()
                    .map(ToString::to_string),
                Some("1979-05-27T07:32:00.999999-07:00".to_string())
            );
            assert_eq!(
                rendered.data["utc_fraction_lower_array"]
                    .as_array()
                    .expect("utc array")
                    .len(),
                2
            );
            assert_eq!(
                rendered.data["signed_int_array"]
                    .as_array()
                    .expect("signed array")
                    .iter()
                    .filter_map(toml::Value::as_integer)
                    .collect::<Vec<_>>(),
                vec![1, 0, -1]
            );
        });
    }

    #[test]
    fn test_parse_rendered_toml_surfaces_parse_failures() {
        let err = parse_rendered_toml("[a]\nvalue = 1\n[a]\nname = \"x\"\n")
            .expect_err("expected TOML parse failure");
        assert_eq!(err.kind, ErrorKind::Parse);
        assert!(err.message.contains("Rendered TOML could not be reparsed"));
        assert!(err.message.contains("duplicate key"));
    }
}
