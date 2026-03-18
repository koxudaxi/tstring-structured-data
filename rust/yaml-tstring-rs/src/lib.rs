use saphyr::{ScalarOwned, YamlOwned};
use saphyr_parser::{ScalarStyle, Tag};
use std::borrow::Cow;
use std::str::FromStr;
use tstring_syntax::{
    BackendError, BackendResult, NormalizedDocument, NormalizedEntry, NormalizedFloat,
    NormalizedKey, NormalizedKeyEntry, NormalizedStream, NormalizedValue, SourcePosition,
    SourceSpan, StreamItem, TemplateInput,
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum YamlProfile {
    V1_2_2,
}

impl YamlProfile {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V1_2_2 => "1.2.2",
        }
    }
}

impl Default for YamlProfile {
    fn default() -> Self {
        Self::V1_2_2
    }
}

impl FromStr for YamlProfile {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "1.2.2" => Ok(Self::V1_2_2),
            other => Err(format!(
                "Unsupported YAML profile {other:?}. Supported profiles: \"1.2.2\"."
            )),
        }
    }
}

#[derive(Clone, Debug)]
pub struct YamlInterpolationNode {
    pub span: SourceSpan,
    pub interpolation_index: usize,
    pub role: String,
}

#[derive(Clone, Debug)]
pub struct YamlTextChunkNode {
    pub span: SourceSpan,
    pub value: String,
}

#[derive(Clone, Debug)]
pub enum YamlChunk {
    Text(YamlTextChunkNode),
    Interpolation(YamlInterpolationNode),
}

#[derive(Clone, Debug)]
pub struct YamlTagNode {
    pub span: SourceSpan,
    pub chunks: Vec<YamlChunk>,
}

#[derive(Clone, Debug)]
pub struct YamlAnchorNode {
    pub span: SourceSpan,
    pub chunks: Vec<YamlChunk>,
}

#[derive(Clone, Debug)]
pub struct YamlPlainScalarNode {
    pub span: SourceSpan,
    pub chunks: Vec<YamlChunk>,
}

#[derive(Clone, Debug)]
pub struct YamlDoubleQuotedScalarNode {
    pub span: SourceSpan,
    pub chunks: Vec<YamlChunk>,
}

#[derive(Clone, Debug)]
pub struct YamlSingleQuotedScalarNode {
    pub span: SourceSpan,
    pub chunks: Vec<YamlChunk>,
}

#[derive(Clone, Debug)]
pub struct YamlBlockScalarNode {
    pub span: SourceSpan,
    pub style: String,
    pub chomping: Option<String>,
    pub indent_indicator: Option<usize>,
    pub chunks: Vec<YamlChunk>,
}

#[derive(Clone, Debug)]
pub struct YamlAliasNode {
    pub span: SourceSpan,
    pub chunks: Vec<YamlChunk>,
}

#[derive(Clone, Debug)]
pub enum YamlKeyValue {
    Scalar(YamlScalarNode),
    Interpolation(YamlInterpolationNode),
    Complex(Box<YamlValueNode>),
}

#[derive(Clone, Debug)]
pub struct YamlKeyNode {
    pub span: SourceSpan,
    pub value: YamlKeyValue,
}

#[derive(Clone, Debug)]
pub struct YamlMappingEntryNode {
    pub span: SourceSpan,
    pub key: YamlKeyNode,
    pub value: YamlValueNode,
}

#[derive(Clone, Debug)]
pub struct YamlMappingNode {
    pub span: SourceSpan,
    pub entries: Vec<YamlMappingEntryNode>,
    pub flow: bool,
}

#[derive(Clone, Debug)]
pub struct YamlSequenceNode {
    pub span: SourceSpan,
    pub items: Vec<YamlValueNode>,
    pub flow: bool,
}

#[derive(Clone, Debug)]
pub struct YamlDecoratedNode {
    pub span: SourceSpan,
    pub value: Box<YamlValueNode>,
    pub tag: Option<YamlTagNode>,
    pub anchor: Option<YamlAnchorNode>,
}

#[derive(Clone, Debug)]
pub struct YamlDocumentNode {
    pub span: SourceSpan,
    pub directives: Vec<String>,
    pub explicit_start: bool,
    pub explicit_end: bool,
    pub value: YamlValueNode,
}

#[derive(Clone, Debug)]
pub struct YamlStreamNode {
    pub span: SourceSpan,
    pub documents: Vec<YamlDocumentNode>,
}

#[derive(Clone, Debug)]
pub enum YamlScalarNode {
    Plain(YamlPlainScalarNode),
    DoubleQuoted(YamlDoubleQuotedScalarNode),
    SingleQuoted(YamlSingleQuotedScalarNode),
    Block(YamlBlockScalarNode),
    Alias(YamlAliasNode),
}

#[derive(Clone, Debug)]
pub enum YamlValueNode {
    Scalar(YamlScalarNode),
    Interpolation(YamlInterpolationNode),
    Mapping(YamlMappingNode),
    Sequence(YamlSequenceNode),
    Decorated(YamlDecoratedNode),
}

pub struct YamlParser {
    items: Vec<StreamItem>,
    index: usize,
}

impl YamlParser {
    #[must_use]
    pub fn new(template: &TemplateInput) -> Self {
        Self {
            items: template.flatten(),
            index: 0,
        }
    }

    #[must_use]
    pub fn from_items(items: Vec<StreamItem>) -> Self {
        Self { items, index: 0 }
    }

    pub fn parse(&mut self) -> BackendResult<YamlDocumentNode> {
        let start = self.mark();
        self.skip_blank_lines();
        self.consume_line_start_tabs()?;
        let value = self.parse_block_node(self.current_line_indent())?;
        self.skip_trailing_document_space();
        self.skip_blank_lines();
        if self.current_kind() != "eof" {
            return Err(self.error("Unexpected trailing YAML content."));
        }
        Ok(YamlDocumentNode {
            span: self.span_from(start),
            directives: Vec::new(),
            explicit_start: false,
            explicit_end: false,
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
        BackendError::parse_at("yaml.parse", message, Some(self.current().span().clone()))
    }

    fn advance(&mut self) {
        if self.current_kind() != "eof" {
            self.index += 1;
        }
    }

    fn is_line_start(&self) -> bool {
        self.index == 0 || self.items[self.index - 1].char() == Some('\n')
    }

    fn current_line_indent(&self) -> usize {
        let mut probe = self.index;
        while probe > 0 && self.items[probe - 1].char() != Some('\n') {
            probe -= 1;
        }
        let mut indent = 0;
        while matches!(self.items[probe].char(), Some(' ')) {
            indent += 1;
            probe += 1;
        }
        indent
    }

    fn skip_blank_lines(&mut self) {
        loop {
            if self.current_kind() == "eof" || !self.is_line_start() {
                return;
            }
            let mut probe = self.index;
            while matches!(self.items[probe].char(), Some(' ')) {
                probe += 1;
            }
            match &self.items[probe] {
                StreamItem::Eof { .. } => {
                    self.index = probe;
                    return;
                }
                StreamItem::Char { ch: '#', .. } => {
                    self.index = probe;
                    while !matches!(self.current_char(), None | Some('\n')) {
                        self.advance();
                    }
                    if self.current_char() == Some('\n') {
                        self.advance();
                    }
                }
                _ if self.items[probe].char() == Some('\n') => {
                    self.index = probe;
                    self.advance();
                }
                _ => return,
            }
        }
    }

    fn skip_trailing_document_space(&mut self) {
        loop {
            while self.current_char() == Some(' ') {
                self.advance();
            }
            if self.current_starts_comment() {
                while !matches!(self.current_char(), None | Some('\n')) {
                    self.advance();
                }
            }
            if self.current_char() == Some('\n') {
                self.advance();
                continue;
            }
            return;
        }
    }

    fn consume_indent(&mut self, indent: usize) -> BackendResult<()> {
        if !self.is_line_start() {
            return Ok(());
        }
        for _ in 0..indent {
            if self.current_char() != Some(' ') {
                return Err(self.error("Incorrect YAML indentation."));
            }
            self.advance();
        }
        Ok(())
    }

    fn parse_block_node(&mut self, indent: usize) -> BackendResult<YamlValueNode> {
        self.skip_blank_lines();
        self.consume_line_start_tabs()?;
        if self.current_kind() == "eof" || self.current_line_indent() < indent {
            return Ok(YamlValueNode::Scalar(YamlScalarNode::Plain(
                null_plain_scalar(),
            )));
        }
        self.consume_indent(indent)?;
        if self.starts_sequence_item() {
            return Ok(YamlValueNode::Sequence(self.parse_block_sequence(indent)?));
        }
        if self.line_has_mapping_key() {
            return Ok(YamlValueNode::Mapping(self.parse_block_mapping(indent)?));
        }
        let decorated = self.parse_decorators()?;
        if decorated.is_some() && self.starts_sequence_item() {
            return Err(self.error("Unexpected trailing YAML content."));
        }
        let value = if decorated.is_some() && self.decorator_line_break_ahead() {
            self.consume_inline_comment_and_line_break();
            self.skip_blank_lines();
            self.consume_line_start_tabs()?;
            let next_indent = self.current_line_indent();
            let has_nested_value = self.current_kind() != "eof"
                && (next_indent >= indent || (indent > 0 && next_indent + 1 == indent));
            if has_nested_value {
                self.parse_block_node(next_indent)?
            } else {
                YamlValueNode::Scalar(YamlScalarNode::Plain(null_plain_scalar()))
            }
        } else {
            self.parse_inline_value(None, indent, false)?
        };
        wrap_decorators(decorated, value)
    }

    fn value_owns_following_lines(value: &YamlValueNode) -> bool {
        match value {
            YamlValueNode::Mapping(node) => !node.flow,
            YamlValueNode::Sequence(node) => !node.flow,
            YamlValueNode::Scalar(YamlScalarNode::Block(_)) => true,
            YamlValueNode::Decorated(node) => Self::value_owns_following_lines(node.value.as_ref()),
            _ => false,
        }
    }

    fn parse_decorators(
        &mut self,
    ) -> BackendResult<Option<(Option<YamlTagNode>, Option<YamlAnchorNode>)>> {
        let mut tag = None;
        let mut anchor = None;
        let mut consumed = false;
        loop {
            if self.current_char() == Some('!') {
                tag = Some(self.parse_tag()?);
                consumed = true;
                self.skip_inline_spaces();
                continue;
            }
            if self.current_char() == Some('&') {
                anchor = Some(self.parse_anchor()?);
                consumed = true;
                self.skip_inline_spaces();
                continue;
            }
            break;
        }
        Ok(consumed.then_some((tag, anchor)))
    }

    fn parse_tag(&mut self) -> BackendResult<YamlTagNode> {
        let start = self.mark();
        self.advance();
        let chunks = if self.current_char() == Some('<') {
            self.parse_verbatim_tag_chunks()?
        } else {
            self.parse_symbol_chunks(&[' ', '\t', '\n', '[', ']', '{', '}', ',', ':'])?
        };
        Ok(YamlTagNode {
            span: self.span_from(start),
            chunks,
        })
    }

    fn parse_anchor(&mut self) -> BackendResult<YamlAnchorNode> {
        let start = self.mark();
        self.advance();
        let chunks = self.parse_symbol_chunks(&[' ', '\t', '\n', '[', ']', '{', '}', ','])?;
        Ok(YamlAnchorNode {
            span: self.span_from(start),
            chunks,
        })
    }

    fn parse_alias(&mut self) -> BackendResult<YamlAliasNode> {
        let start = self.mark();
        self.advance();
        let chunks = self.parse_symbol_chunks(&[' ', '\t', '\n', '[', ']', '{', '}', ','])?;
        Ok(YamlAliasNode {
            span: self.span_from(start),
            chunks,
        })
    }

    fn parse_symbol_chunks(&mut self, stop_chars: &[char]) -> BackendResult<Vec<YamlChunk>> {
        let mut chunks = Vec::new();
        let mut buffer = String::new();
        while self.current_kind() != "eof" {
            if self.current_kind() == "interpolation" {
                self.flush_buffer(&mut buffer, &mut chunks);
                chunks.push(YamlChunk::Interpolation(
                    self.consume_interpolation("metadata_fragment")?,
                ));
                continue;
            }
            let Some(ch) = self.current_char() else {
                break;
            };
            if stop_chars.contains(&ch) {
                break;
            }
            buffer.push(ch);
            self.advance();
        }
        self.flush_buffer(&mut buffer, &mut chunks);
        Ok(chunks)
    }

    fn parse_verbatim_tag_chunks(&mut self) -> BackendResult<Vec<YamlChunk>> {
        let mut chunks = Vec::new();
        let mut buffer = String::new();
        buffer.push('<');
        self.advance();

        while self.current_kind() != "eof" {
            if self.current_kind() == "interpolation" {
                self.flush_buffer(&mut buffer, &mut chunks);
                chunks.push(YamlChunk::Interpolation(
                    self.consume_interpolation("metadata_fragment")?,
                ));
                continue;
            }

            let Some(ch) = self.current_char() else {
                break;
            };
            buffer.push(ch);
            self.advance();
            if ch == '>' {
                self.flush_buffer(&mut buffer, &mut chunks);
                return Ok(chunks);
            }
        }

        Err(self.error("Unterminated YAML verbatim tag."))
    }

    fn skip_inline_spaces(&mut self) {
        while matches!(self.current_char(), Some(' ' | '\t')) {
            self.advance();
        }
    }

    fn skip_flow_separation(&mut self) {
        self.skip_flow_separation_with_breaks();
    }

    fn skip_flow_separation_with_breaks(&mut self) -> bool {
        let mut saw_line_break = false;
        loop {
            while matches!(self.current_char(), Some(' ' | '\t' | '\r')) {
                self.advance();
            }
            if self.current_starts_comment() {
                while !matches!(self.current_char(), None | Some('\n')) {
                    self.advance();
                }
            }
            if self.current_char() == Some('\n') {
                saw_line_break = true;
                self.advance();
                continue;
            }
            break;
        }
        saw_line_break
    }

    fn starts_sequence_item(&self) -> bool {
        self.starts_sequence_item_at(self.index)
    }

    fn starts_sequence_item_at(&self, mut probe: usize) -> bool {
        while matches!(self.items.get(probe).and_then(StreamItem::char), Some(' ')) {
            probe += 1;
        }
        matches!(self.items.get(probe).and_then(StreamItem::char), Some('-'))
            && matches!(
                self.items.get(probe + 1).and_then(StreamItem::char),
                Some(' ' | '\t' | '\n')
            )
    }

    fn peek_char(&self, offset: usize) -> Option<char> {
        self.items
            .get(self.index + offset)
            .and_then(StreamItem::char)
    }

    fn line_has_mapping_key(&self) -> bool {
        self.line_has_mapping_key_at(self.index)
    }

    fn line_has_mapping_key_at(&self, probe: usize) -> bool {
        if matches!(self.items.get(probe).and_then(StreamItem::char), Some('?'))
            && matches!(
                self.items.get(probe + 1).and_then(StreamItem::char),
                Some(' ' | '\t' | '\n')
            )
        {
            return true;
        }
        let mut parser = Self {
            items: self.items.clone(),
            index: probe,
        };
        if parser.parse_key().is_err() {
            return false;
        }
        parser.skip_key_value_spaces();
        if parser.current_char() != Some(':') {
            return false;
        }
        matches!(parser.peek_char(1), Some(' ' | '\t' | '\n') | None)
    }

    fn skip_key_value_spaces(&mut self) {
        while matches!(self.current_char(), Some(' ' | '\t')) {
            self.advance();
        }
    }

    fn decorator_line_break_ahead(&self) -> bool {
        let mut probe = self.index;
        while matches!(
            self.items.get(probe).and_then(StreamItem::char),
            Some(' ' | '\t')
        ) {
            probe += 1;
        }
        if self.items.get(probe).and_then(StreamItem::char) == Some('#') {
            while !matches!(
                self.items.get(probe).and_then(StreamItem::char),
                None | Some('\n')
            ) {
                probe += 1;
            }
        }
        matches!(self.items.get(probe).and_then(StreamItem::char), Some('\n'))
            || matches!(self.items.get(probe), Some(StreamItem::Eof { .. }))
    }

    fn consume_inline_comment_and_line_break(&mut self) {
        self.skip_inline_spaces();
        if self.current_starts_comment() {
            while !matches!(self.current_char(), None | Some('\n')) {
                self.advance();
            }
        }
        if self.current_char() == Some('\n') {
            self.advance();
        }
    }

    fn consume_line_end_required(&mut self) -> BackendResult<()> {
        if self.current_kind() == "eof" || self.is_line_start() {
            return Ok(());
        }
        self.skip_inline_spaces();
        if self.current_starts_comment() {
            while !matches!(self.current_char(), None | Some('\n')) {
                self.advance();
            }
        }
        match self.current_char() {
            Some('\n') => {
                self.advance();
                Ok(())
            }
            None => Ok(()),
            _ if self.current_kind() == "eof" => Ok(()),
            _ => Err(self.error("Unexpected trailing YAML content.")),
        }
    }

    fn consume_line_start_tabs(&mut self) -> BackendResult<()> {
        if !self.is_line_start() || self.current_char() != Some('\t') {
            return Ok(());
        }

        let mut probe = self.index;
        while self.items.get(probe).and_then(StreamItem::char) == Some('\t') {
            probe += 1;
        }

        if self.starts_sequence_item_at(probe) || self.line_has_mapping_key_at(probe) {
            return Err(self.error("Tabs are not allowed as YAML indentation."));
        }

        self.index = probe;
        Ok(())
    }

    fn current_starts_comment(&self) -> bool {
        self.current_char() == Some('#')
            && (self.index == 0
                || matches!(
                    self.items
                        .get(self.index.saturating_sub(1))
                        .and_then(StreamItem::char),
                    Some(' ' | '\t' | '\n' | '\r')
                ))
    }

    fn parse_key_value_separator(&mut self) -> BackendResult<()> {
        self.skip_key_value_spaces();
        self.consume_char(':')?;
        if self.current_char() == Some('\t') {
            let mut probe = self.index;
            while self.items.get(probe).and_then(StreamItem::char) == Some('\t') {
                probe += 1;
            }
            if !matches!(
                self.items.get(probe).and_then(StreamItem::char),
                Some(
                    ' ' | '\n'
                        | '\r'
                        | '#'
                        | '['
                        | '{'
                        | '"'
                        | '\''
                        | '!'
                        | '&'
                        | '*'
                        | '|'
                        | '>'
                        | '?'
                ) | None
            ) {
                return Err(self.error("Tabs are not allowed as YAML indentation."));
            }
        }
        Ok(())
    }

    fn classify_key_value(&self, start: SourcePosition, value: YamlValueNode) -> YamlKeyNode {
        let value = match value {
            YamlValueNode::Interpolation(node) => YamlKeyValue::Interpolation(node),
            YamlValueNode::Scalar(
                node @ (YamlScalarNode::Plain(_)
                | YamlScalarNode::DoubleQuoted(_)
                | YamlScalarNode::SingleQuoted(_)),
            ) => YamlKeyValue::Scalar(node),
            other => YamlKeyValue::Complex(Box::new(other)),
        };
        YamlKeyNode {
            span: self.span_from(start),
            value,
        }
    }

    fn parse_block_sequence(&mut self, indent: usize) -> BackendResult<YamlSequenceNode> {
        let start = self.mark();
        let mut items = Vec::new();
        let mut first_item = true;
        loop {
            if !first_item || self.is_line_start() {
                self.consume_indent(indent)?;
            }
            first_item = false;
            if !self.starts_sequence_item() {
                break;
            }
            self.advance();
            if matches!(self.current_char(), Some(' ' | '\t')) {
                self.skip_inline_spaces();
            }
            if self.current_starts_comment() {
                while !matches!(self.current_char(), None | Some('\n')) {
                    self.advance();
                }
                if self.current_char() == Some('\n') {
                    self.advance();
                }
                self.skip_blank_lines();
                if self.is_line_start() && self.current_char() == Some('\t') {
                    self.consume_line_start_tabs()?;
                }
                let item = if self.current_kind() == "eof" || self.current_line_indent() <= indent {
                    YamlValueNode::Scalar(YamlScalarNode::Plain(null_plain_scalar()))
                } else {
                    self.parse_block_node(self.current_line_indent())?
                };
                let owns_following_lines = Self::value_owns_following_lines(&item);
                items.push(item);
                if !owns_following_lines && self.current_kind() != "eof" {
                    self.consume_line_end_required()?;
                }
            } else if self.current_kind() == "eof" || self.current_char() == Some('\n') {
                if self.current_char() == Some('\n') {
                    self.advance();
                }
                self.skip_blank_lines();
                let item = self.parse_block_node(self.current_line_indent())?;
                let owns_following_lines = Self::value_owns_following_lines(&item);
                items.push(item);
                if !owns_following_lines {
                    self.consume_line_end_required()?;
                }
            } else if self.starts_sequence_item() {
                items.push(YamlValueNode::Sequence(
                    self.parse_compact_sequence(indent + 2)?,
                ));
            } else if self.line_has_mapping_key() {
                items.push(YamlValueNode::Mapping(
                    self.parse_compact_mapping(indent + 2)?,
                ));
            } else {
                let item = self.parse_inline_value(None, indent, true)?;
                let owns_following_lines = Self::value_owns_following_lines(&item);
                items.push(item);
                if !owns_following_lines {
                    self.consume_line_end_required()?;
                }
            }
            self.skip_blank_lines();
            if self.current_kind() == "eof"
                || self.current_line_indent() != indent
                || !self.starts_sequence_item()
            {
                break;
            }
        }
        Ok(YamlSequenceNode {
            span: self.span_from(start),
            items,
            flow: false,
        })
    }

    fn parse_compact_sequence(&mut self, indent: usize) -> BackendResult<YamlSequenceNode> {
        let start = self.mark();
        let mut items = Vec::new();
        let mut first_item = true;
        loop {
            if !first_item || self.is_line_start() {
                self.consume_indent(indent)?;
            }
            first_item = false;
            if !self.starts_sequence_item() {
                break;
            }
            self.advance();
            if matches!(self.current_char(), Some(' ' | '\t')) {
                self.skip_inline_spaces();
            }
            if self.current_starts_comment() {
                while !matches!(self.current_char(), None | Some('\n')) {
                    self.advance();
                }
                if self.current_char() == Some('\n') {
                    self.advance();
                }
                self.skip_blank_lines();
                if self.is_line_start() && self.current_char() == Some('\t') {
                    self.consume_line_start_tabs()?;
                }
                let item = if self.current_kind() == "eof" || self.current_line_indent() <= indent {
                    YamlValueNode::Scalar(YamlScalarNode::Plain(null_plain_scalar()))
                } else {
                    self.parse_block_node(self.current_line_indent())?
                };
                let owns_following_lines = Self::value_owns_following_lines(&item);
                items.push(item);
                if !owns_following_lines && self.current_kind() != "eof" {
                    self.consume_line_end_required()?;
                }
            } else if self.current_kind() == "eof" || self.current_char() == Some('\n') {
                if self.current_char() == Some('\n') {
                    self.advance();
                }
                self.skip_blank_lines();
                let item = self.parse_block_node(self.current_line_indent())?;
                let owns_following_lines = Self::value_owns_following_lines(&item);
                items.push(item);
                if !owns_following_lines {
                    self.consume_line_end_required()?;
                }
            } else if self.starts_sequence_item() {
                items.push(YamlValueNode::Sequence(
                    self.parse_compact_sequence(indent + 2)?,
                ));
            } else if self.line_has_mapping_key() {
                items.push(YamlValueNode::Mapping(
                    self.parse_compact_mapping(indent + 2)?,
                ));
            } else {
                let item = self.parse_inline_value(None, indent, true)?;
                let owns_following_lines = Self::value_owns_following_lines(&item);
                items.push(item);
                if !owns_following_lines {
                    self.consume_line_end_required()?;
                }
            }
            self.skip_blank_lines();
            if self.current_kind() == "eof"
                || self.current_line_indent() < indent
                || !self.starts_sequence_item()
            {
                break;
            }
        }
        Ok(YamlSequenceNode {
            span: self.span_from(start),
            items,
            flow: false,
        })
    }

    fn parse_compact_mapping(&mut self, indent: usize) -> BackendResult<YamlMappingNode> {
        let start = self.mark();
        let mut entries = Vec::new();
        loop {
            let entry_start = self.mark();
            let explicit_key =
                self.current_char() == Some('?') && matches!(self.peek_char(1), Some(' ' | '\n'));
            let key = if explicit_key {
                self.parse_explicit_key(indent)?
            } else {
                self.parse_key()?
            };
            let value = if explicit_key {
                self.parse_explicit_mapping_entry_value(indent)?
            } else {
                self.parse_key_value_separator()?;
                self.parse_mapping_value_after_separator(indent, false)?
            };
            if !Self::value_owns_following_lines(&value) {
                self.consume_line_end_required()?;
            }
            entries.push(YamlMappingEntryNode {
                span: self.span_from(entry_start),
                key,
                value,
            });
            self.skip_blank_lines();
            if self.current_kind() == "eof"
                || self.current_line_indent() != indent
                || self.starts_sequence_item()
                || !self.line_has_mapping_key()
            {
                break;
            }
            self.consume_indent(indent)?;
        }
        Ok(YamlMappingNode {
            span: self.span_from(start),
            entries,
            flow: false,
        })
    }

    fn parse_block_mapping(&mut self, indent: usize) -> BackendResult<YamlMappingNode> {
        let start = self.mark();
        let mut entries = Vec::new();
        let mut first_entry = true;
        loop {
            if !first_entry {
                self.consume_indent(indent)?;
            }
            first_entry = false;
            let entry_start = self.mark();
            let explicit_key =
                self.current_char() == Some('?') && matches!(self.peek_char(1), Some(' ' | '\n'));
            let key = if explicit_key {
                self.parse_explicit_key(indent)?
            } else {
                self.parse_key()?
            };
            let value = if explicit_key {
                self.parse_explicit_mapping_entry_value(indent)?
            } else {
                self.parse_key_value_separator()?;
                self.parse_mapping_value_after_separator(indent, false)?
            };
            if !Self::value_owns_following_lines(&value) {
                self.consume_line_end_required()?;
            }
            entries.push(YamlMappingEntryNode {
                span: self.span_from(entry_start),
                key,
                value,
            });
            self.skip_blank_lines();
            if self.current_kind() == "eof"
                || self.current_line_indent() != indent
                || self.starts_sequence_item()
                || !self.line_has_mapping_key()
            {
                break;
            }
        }
        Ok(YamlMappingNode {
            span: self.span_from(start),
            entries,
            flow: false,
        })
    }

    fn parse_explicit_mapping_entry_value(
        &mut self,
        indent: usize,
    ) -> BackendResult<YamlValueNode> {
        if self.current_kind() == "eof"
            || (self.is_line_start() && self.current_line_indent() < indent)
        {
            return Ok(YamlValueNode::Scalar(YamlScalarNode::Plain(
                null_plain_scalar(),
            )));
        }

        self.consume_indent(indent)?;
        if self.current_char() != Some(':') {
            return Ok(YamlValueNode::Scalar(YamlScalarNode::Plain(
                null_plain_scalar(),
            )));
        }

        self.parse_key_value_separator()?;
        self.parse_mapping_value_after_separator(indent, true)
    }

    fn parse_mapping_value_after_separator(
        &mut self,
        indent: usize,
        allow_inline_compact_sequence: bool,
    ) -> BackendResult<YamlValueNode> {
        if matches!(self.current_char(), Some(' ' | '\t')) {
            if matches!(self.current_char(), Some(' ' | '\t')) {
                self.skip_inline_spaces();
            }
            return self.parse_mapping_value(indent, allow_inline_compact_sequence);
        }
        if matches!(self.current_kind(), "eof") || self.current_char() == Some('\n') {
            if self.current_char() == Some('\n') {
                self.advance();
            }
            self.skip_blank_lines();
            self.consume_line_start_tabs()?;
            if self.starts_sequence_item() && self.current_line_indent() == indent {
                return Ok(YamlValueNode::Sequence(self.parse_block_sequence(indent)?));
            }
            if self.current_kind() == "eof" || self.current_line_indent() <= indent {
                return Ok(YamlValueNode::Scalar(YamlScalarNode::Plain(
                    null_plain_scalar(),
                )));
            }
            return self.parse_block_node(self.current_line_indent());
        }
        self.parse_inline_value(None, indent, true)
    }

    fn parse_mapping_value(
        &mut self,
        indent: usize,
        allow_inline_compact_sequence: bool,
    ) -> BackendResult<YamlValueNode> {
        if self.current_starts_comment() {
            while !matches!(self.current_char(), None | Some('\n')) {
                self.advance();
            }
            if self.current_char() == Some('\n') {
                self.advance();
            }
            self.skip_blank_lines();
            if self.is_line_start() && self.current_char() == Some('\t') {
                self.consume_line_start_tabs()?;
            }
            if self.starts_sequence_item() && self.current_line_indent() == indent {
                return Ok(YamlValueNode::Sequence(self.parse_block_sequence(indent)?));
            }
            if self.current_kind() == "eof" || self.current_line_indent() <= indent {
                return Ok(YamlValueNode::Scalar(YamlScalarNode::Plain(
                    null_plain_scalar(),
                )));
            }
            return self.parse_block_node(self.current_line_indent());
        }
        if self.starts_sequence_item() {
            if self.is_line_start() && self.current_line_indent() == indent {
                return Ok(YamlValueNode::Sequence(self.parse_block_sequence(indent)?));
            }
            if allow_inline_compact_sequence {
                return Ok(YamlValueNode::Sequence(
                    self.parse_compact_sequence(indent + 2)?,
                ));
            }
            return Err(self.error("Unexpected trailing YAML content."));
        }
        if allow_inline_compact_sequence && self.line_has_mapping_key() {
            return Ok(YamlValueNode::Mapping(
                self.parse_compact_mapping(indent + 2)?,
            ));
        }
        if matches!(self.current_char(), Some(' ' | '\t')) {
            self.skip_inline_spaces();
            if self.current_starts_comment() {
                while !matches!(self.current_char(), None | Some('\n')) {
                    self.advance();
                }
                if self.current_char() == Some('\n') {
                    self.advance();
                }
                self.skip_blank_lines();
                if self.is_line_start() && self.current_char() == Some('\t') {
                    self.consume_line_start_tabs()?;
                }
                if self.starts_sequence_item() && self.current_line_indent() == indent {
                    return Ok(YamlValueNode::Sequence(self.parse_block_sequence(indent)?));
                }
                if self.current_kind() == "eof" || self.current_line_indent() <= indent {
                    return Ok(YamlValueNode::Scalar(YamlScalarNode::Plain(
                        null_plain_scalar(),
                    )));
                }
                return self.parse_block_node(self.current_line_indent());
            }
            if self.starts_sequence_item() {
                if self.is_line_start() && self.current_line_indent() == indent {
                    return Ok(YamlValueNode::Sequence(self.parse_block_sequence(indent)?));
                }
                if allow_inline_compact_sequence {
                    return Ok(YamlValueNode::Sequence(
                        self.parse_compact_sequence(indent + 2)?,
                    ));
                }
                return Err(self.error("Unexpected trailing YAML content."));
            }
            if allow_inline_compact_sequence && self.line_has_mapping_key() {
                return Ok(YamlValueNode::Mapping(
                    self.parse_compact_mapping(indent + 2)?,
                ));
            }
            return self.parse_inline_value(None, indent, true);
        }
        if self.current_kind() == "eof" || self.current_char() == Some('\n') {
            if self.current_char() == Some('\n') {
                self.advance();
            }
            self.skip_blank_lines();
            if self.is_line_start() && self.current_char() == Some('\t') {
                self.consume_line_start_tabs()?;
            }
            if self.starts_sequence_item() && self.current_line_indent() == indent {
                return Ok(YamlValueNode::Sequence(self.parse_block_sequence(indent)?));
            }
            if self.current_kind() == "eof" || self.current_line_indent() <= indent {
                return Ok(YamlValueNode::Scalar(YamlScalarNode::Plain(
                    null_plain_scalar(),
                )));
            }
            return self.parse_block_node(self.current_line_indent());
        }
        self.parse_inline_value(None, indent, true)
    }

    fn parse_explicit_key(&mut self, indent: usize) -> BackendResult<YamlKeyNode> {
        let start = self.mark();
        self.consume_char('?')?;
        if matches!(self.current_char(), Some(' ' | '\t')) {
            self.skip_inline_spaces();
        }
        let value = if self.current_starts_comment() {
            while !matches!(self.current_char(), None | Some('\n')) {
                self.advance();
            }
            if self.current_char() == Some('\n') {
                self.advance();
            }
            self.skip_blank_lines();
            if self.is_line_start() && self.current_char() == Some('\t') {
                self.consume_line_start_tabs()?;
            }
            if self.current_kind() == "eof" || self.current_char() == Some(':') {
                YamlValueNode::Scalar(YamlScalarNode::Plain(null_plain_scalar()))
            } else {
                self.parse_block_node(self.current_line_indent())?
            }
        } else if self.current_kind() == "eof" || self.current_char() == Some('\n') {
            if self.current_char() == Some('\n') {
                self.advance();
            }
            self.skip_blank_lines();
            if self.current_kind() == "eof" || self.current_char() == Some(':') {
                YamlValueNode::Scalar(YamlScalarNode::Plain(null_plain_scalar()))
            } else {
                self.parse_block_node(self.current_line_indent())?
            }
        } else if self.starts_sequence_item() {
            YamlValueNode::Sequence(self.parse_compact_sequence(indent + 2)?)
        } else if self.line_has_mapping_key() {
            YamlValueNode::Mapping(self.parse_compact_mapping(indent + 2)?)
        } else {
            let value = self.parse_inline_value(None, indent, true)?;
            self.consume_line_end_required()?;
            value
        };
        Ok(self.classify_key_value(start, value))
    }

    fn parse_key(&mut self) -> BackendResult<YamlKeyNode> {
        let start = self.mark();
        let key_start = self.index;
        let value = self.parse_inline_value(Some(&[':']), 0, false)?;
        self.ensure_single_line_implicit_key(key_start)?;
        Ok(self.classify_key_value(start, value))
    }

    fn parse_inline_value(
        &mut self,
        stop_chars: Option<&[char]>,
        parent_indent: usize,
        requires_indented_continuation: bool,
    ) -> BackendResult<YamlValueNode> {
        let decorated = self.parse_decorators()?;
        if decorated.is_some() && self.starts_sequence_item() {
            return Err(self.error("Unexpected trailing YAML content."));
        }
        if decorated.is_some() && self.decorator_line_break_ahead() {
            self.consume_inline_comment_and_line_break();
            self.skip_blank_lines();
            self.consume_line_start_tabs()?;
            let has_nested_value = self.current_kind() != "eof"
                && (self.current_line_indent() > parent_indent
                    || (self.current_line_indent() == parent_indent
                        && self.starts_sequence_item()));
            let value = if has_nested_value {
                self.parse_block_node(self.current_line_indent())?
            } else {
                YamlValueNode::Scalar(YamlScalarNode::Plain(YamlPlainScalarNode {
                    span: SourceSpan::point(0, 0),
                    chunks: vec![YamlChunk::Text(YamlTextChunkNode {
                        span: SourceSpan::point(0, 0),
                        value: "null".to_owned(),
                    })],
                }))
            };
            return wrap_decorators(decorated, value);
        }
        if decorated.is_some() && self.inline_value_terminator(stop_chars) {
            return wrap_decorators(
                decorated,
                YamlValueNode::Scalar(YamlScalarNode::Plain(empty_plain_scalar())),
            );
        }
        let value = if self.current_kind() == "interpolation" {
            let interpolation = self.consume_interpolation("value")?;
            if self.inline_value_terminator(stop_chars) {
                YamlValueNode::Interpolation(interpolation)
            } else {
                YamlValueNode::Scalar(YamlScalarNode::Plain(self.parse_plain_scalar(
                    stop_chars,
                    Some(vec![YamlChunk::Interpolation(interpolation)]),
                    false,
                    parent_indent,
                    requires_indented_continuation,
                )?))
            }
        } else if self.current_char() == Some('[') {
            YamlValueNode::Sequence(self.parse_flow_sequence()?)
        } else if self.current_char() == Some('{') {
            YamlValueNode::Mapping(self.parse_flow_mapping()?)
        } else if self.current_char() == Some('"') {
            YamlValueNode::Scalar(YamlScalarNode::DoubleQuoted(
                self.parse_double_quoted(parent_indent, requires_indented_continuation)?,
            ))
        } else if self.current_char() == Some('\'') {
            YamlValueNode::Scalar(YamlScalarNode::SingleQuoted(self.parse_single_quoted()?))
        } else if self.current_char() == Some('*') {
            YamlValueNode::Scalar(YamlScalarNode::Alias(self.parse_alias()?))
        } else if matches!(self.current_char(), Some('|' | '>')) {
            YamlValueNode::Scalar(YamlScalarNode::Block(
                self.parse_block_scalar(parent_indent)?,
            ))
        } else if matches!(self.current_char(), Some(',' | ']' | '}')) {
            return Err(self.error("Expected a YAML value."));
        } else {
            YamlValueNode::Scalar(YamlScalarNode::Plain(self.parse_plain_scalar(
                stop_chars,
                None,
                false,
                parent_indent,
                requires_indented_continuation,
            )?))
        };
        wrap_decorators(decorated, value)
    }

    fn inline_value_terminator(&self, stop_chars: Option<&[char]>) -> bool {
        let mut probe = self.index;
        while matches!(self.items[probe].char(), Some(' ' | '\t')) {
            probe += 1;
        }
        match &self.items[probe] {
            StreamItem::Eof { .. } => true,
            StreamItem::Char { ch, .. } => {
                stop_chars.is_some_and(|chars| chars.contains(ch)) || matches!(ch, '\n' | '#')
            }
            _ => false,
        }
    }

    fn parse_flow_sequence(&mut self) -> BackendResult<YamlSequenceNode> {
        let start = self.mark();
        let line_value_start = self.is_line_value_start();
        self.consume_char('[')?;
        let mut items = Vec::new();
        self.skip_flow_separation();
        if self.current_char() == Some(']') {
            self.advance();
            return Ok(YamlSequenceNode {
                span: self.span_from(start),
                items,
                flow: true,
            });
        }
        loop {
            if self.current_char() == Some(',') {
                return Err(self.error("Expected a YAML value."));
            }
            if self.current_char() == Some('-') && matches!(self.peek_char(1), Some(',' | ']')) {
                return Err(self.error("Expected a YAML value."));
            }
            items.push(self.parse_flow_sequence_item()?);
            self.skip_flow_separation();
            if self.current_char() == Some(']') {
                self.advance();
                break;
            }
            self.consume_char(',')?;
            let saw_line_break = self.skip_flow_separation_with_breaks();
            if saw_line_break
                && !line_value_start
                && self.current_char() != Some(']')
                && self.current_line_indent() == 0
            {
                return Err(self.error("Unexpected trailing YAML content."));
            }
            if self.current_char() == Some(']') {
                self.advance();
                break;
            }
        }
        Ok(YamlSequenceNode {
            span: self.span_from(start),
            items,
            flow: true,
        })
    }

    fn is_line_value_start(&self) -> bool {
        let mut probe = self.index;
        while probe > 0 && self.items[probe - 1].char() != Some('\n') {
            probe -= 1;
        }
        while probe < self.index {
            match self.items[probe].char() {
                Some(' ' | '\t') => probe += 1,
                _ => return false,
            }
        }
        true
    }

    fn parse_flow_sequence_item(&mut self) -> BackendResult<YamlValueNode> {
        let entry_start = self.mark();
        let explicit =
            self.current_char() == Some('?') && matches!(self.peek_char(1), Some(' ' | '[' | '{'));
        if explicit {
            self.consume_char('?')?;
            self.skip_inline_spaces();
        }

        let key_value = self.parse_inline_value(Some(&[':', ',', ']']), 0, false)?;
        let saw_line_break = self.skip_flow_separation_with_breaks();
        if self.current_char() != Some(':') {
            if explicit {
                return Err(self.error("Expected ':' in YAML template."));
            }
            return Ok(key_value);
        }
        if saw_line_break {
            return Err(self.error("Expected ':' in YAML template."));
        }

        let key = self.classify_key_value(entry_start.clone(), key_value);
        self.parse_key_value_separator()?;
        self.skip_flow_separation();
        let value = if matches!(self.current_char(), Some(',' | ']')) {
            YamlValueNode::Scalar(YamlScalarNode::Plain(null_plain_scalar()))
        } else {
            self.parse_inline_value(Some(&[',', ']']), 0, false)?
        };
        let pair_span = self.span_from(entry_start);
        Ok(YamlValueNode::Mapping(YamlMappingNode {
            span: pair_span.clone(),
            entries: vec![YamlMappingEntryNode {
                span: pair_span,
                key,
                value,
            }],
            flow: true,
        }))
    }

    fn parse_flow_mapping(&mut self) -> BackendResult<YamlMappingNode> {
        let start = self.mark();
        self.consume_char('{')?;
        let mut entries = Vec::new();
        self.skip_flow_separation();
        if self.current_char() == Some(',') {
            return Err(self.error("Expected ':' in YAML template."));
        }
        if self.current_char() == Some('}') {
            self.advance();
            return Ok(YamlMappingNode {
                span: self.span_from(start),
                entries,
                flow: true,
            });
        }
        loop {
            let entry_start = self.mark();
            let key = self.parse_flow_key()?;
            self.skip_flow_separation();
            let value = if matches!(self.current_char(), Some(',' | '}')) {
                if self.current_char() == Some('}') && flow_implicit_plain_key_needs_separator(&key)
                {
                    return Err(self.error("Expected ':' in YAML template."));
                }
                YamlValueNode::Scalar(YamlScalarNode::Plain(null_plain_scalar()))
            } else {
                self.parse_key_value_separator()?;
                self.skip_flow_separation();
                if matches!(self.current_char(), Some(',' | '}')) {
                    YamlValueNode::Scalar(YamlScalarNode::Plain(null_plain_scalar()))
                } else {
                    self.parse_inline_value(Some(&[',', '}']), 0, false)?
                }
            };
            entries.push(YamlMappingEntryNode {
                span: self.span_from(entry_start),
                key,
                value,
            });
            self.skip_flow_separation();
            if self.current_char() == Some('}') {
                self.advance();
                break;
            }
            self.consume_char(',')?;
            self.skip_flow_separation();
            if self.current_char() == Some(',') {
                return Err(self.error("Expected ':' in YAML template."));
            }
            if self.current_char() == Some('}') {
                self.advance();
                break;
            }
        }
        Ok(YamlMappingNode {
            span: self.span_from(start),
            entries,
            flow: true,
        })
    }

    fn parse_flow_key(&mut self) -> BackendResult<YamlKeyNode> {
        let start = self.mark();
        let explicit =
            self.current_char() == Some('?') && matches!(self.peek_char(1), Some(' ' | '[' | '{'));
        if explicit {
            self.consume_char('?')?;
            self.skip_inline_spaces();
        }
        let value = self.parse_inline_value(Some(&[':', ',', '}']), 0, false)?;
        Ok(self.classify_key_value(start, value))
    }

    fn ensure_single_line_implicit_key(&self, start_index: usize) -> BackendResult<()> {
        let saw_line_break = self.items[start_index..self.index]
            .iter()
            .any(|item| item.char() == Some('\n'));
        if saw_line_break {
            return Err(BackendError::parse_at(
                "yaml.parse",
                "Implicit YAML keys must be on a single line.",
                Some(self.current().span().clone()),
            ));
        }
        Ok(())
    }

    fn parse_double_quoted(
        &mut self,
        parent_indent: usize,
        requires_indented_continuation: bool,
    ) -> BackendResult<YamlDoubleQuotedScalarNode> {
        let start = self.mark();
        self.consume_char('"')?;
        let mut chunks = Vec::new();
        let mut buffer = String::new();
        loop {
            if self.current_kind() == "eof" {
                return Err(self.error("Unterminated YAML double-quoted scalar."));
            }
            if self.current_kind() == "interpolation" {
                self.flush_buffer(&mut buffer, &mut chunks);
                chunks.push(YamlChunk::Interpolation(
                    self.consume_interpolation("string_fragment")?,
                ));
                continue;
            }
            if self.current_char() == Some('"') {
                self.flush_buffer(&mut buffer, &mut chunks);
                self.advance();
                break;
            }
            if matches!(self.current_char(), Some('\r' | '\n')) {
                buffer.push_str(&self.parse_quoted_line_folding(
                    '"',
                    parent_indent,
                    requires_indented_continuation,
                )?);
                continue;
            }
            if self.current_char() == Some('\\') {
                buffer.push_str(&self.parse_double_quoted_escape()?);
                continue;
            }
            if let Some(ch) = self.current_char() {
                buffer.push(ch);
                self.advance();
            }
        }
        Ok(YamlDoubleQuotedScalarNode {
            span: self.span_from(start),
            chunks,
        })
    }

    fn parse_quoted_line_folding(
        &mut self,
        terminator: char,
        parent_indent: usize,
        requires_indented_continuation: bool,
    ) -> BackendResult<String> {
        let mut breaks = 0usize;
        loop {
            if self.current_char() == Some('\r') {
                self.advance();
            }
            if self.current_char() != Some('\n') {
                return Err(self.error("Invalid YAML quoted-scalar line break."));
            }
            self.advance();
            breaks += 1;
            let mut indent = 0usize;
            while matches!(self.current_char(), Some(' ' | '\t')) {
                indent += 1;
                self.advance();
            }
            let has_required_indent = if requires_indented_continuation {
                indent > parent_indent
            } else {
                indent >= parent_indent
            };
            if !has_required_indent
                && !matches!(self.current_char(), Some('\r' | '\n'))
                && self.current_char() != Some(terminator)
            {
                return Err(self.error("Invalid YAML quoted-scalar line break."));
            }
            if !matches!(self.current_char(), Some('\r' | '\n')) {
                break;
            }
        }
        if breaks == 1 {
            Ok(" ".to_owned())
        } else {
            Ok("\n".repeat(breaks - 1))
        }
    }

    fn parse_single_quoted(&mut self) -> BackendResult<YamlSingleQuotedScalarNode> {
        let start = self.mark();
        self.consume_char('\'')?;
        let mut chunks = Vec::new();
        let mut buffer = String::new();
        loop {
            if self.current_kind() == "eof" {
                return Err(self.error("Unterminated YAML single-quoted scalar."));
            }
            if self.current_kind() == "interpolation" {
                self.flush_buffer(&mut buffer, &mut chunks);
                chunks.push(YamlChunk::Interpolation(
                    self.consume_interpolation("string_fragment")?,
                ));
                continue;
            }
            if self.current_char() == Some('\'') {
                if self.peek_char(1) == Some('\'') {
                    buffer.push('\'');
                    self.advance();
                    self.advance();
                    continue;
                }
                self.flush_buffer(&mut buffer, &mut chunks);
                self.advance();
                break;
            }
            if let Some(ch) = self.current_char() {
                buffer.push(ch);
                self.advance();
            }
        }
        Ok(YamlSingleQuotedScalarNode {
            span: self.span_from(start),
            chunks,
        })
    }

    fn parse_plain_scalar(
        &mut self,
        stop_chars: Option<&[char]>,
        leading: Option<Vec<YamlChunk>>,
        key_mode: bool,
        parent_indent: usize,
        requires_indented_continuation: bool,
    ) -> BackendResult<YamlPlainScalarNode> {
        let start = self.mark();
        let mut chunks = leading.unwrap_or_default();
        let mut buffer = String::new();
        while self.current_kind() != "eof" {
            if self.current_kind() == "interpolation" {
                self.flush_buffer(&mut buffer, &mut chunks);
                chunks.push(YamlChunk::Interpolation(
                    self.consume_interpolation("string_fragment")?,
                ));
                continue;
            }
            let Some(ch) = self.current_char() else {
                break;
            };
            if ch == '#' && buffer.is_empty() {
                return Err(self.error("Expected a YAML value."));
            }
            if ch == '#'
                && self
                    .items
                    .get(self.index.wrapping_sub(1))
                    .and_then(StreamItem::char)
                    .is_none_or(char::is_whitespace)
            {
                break;
            }
            if ch == '\n' {
                if !key_mode {
                    if stop_chars.is_none()
                        && self.consume_plain_scalar_continuation(
                            &mut buffer,
                            parent_indent,
                            requires_indented_continuation,
                        )
                    {
                        continue;
                    }
                    if let Some(stop_chars) = stop_chars {
                        if self.consume_flow_plain_scalar_continuation(
                            &mut buffer,
                            stop_chars,
                            parent_indent,
                        ) {
                            continue;
                        }
                    }
                }
                break;
            }
            if stop_chars.is_some_and(|chars| chars.contains(&ch)) {
                let colon_without_separator = ch == ':'
                    && !matches!(self.peek_char(1), Some(' ' | '\t' | '\n' | ',' | ']' | '}'));
                if !colon_without_separator
                    && (!key_mode || matches!(self.peek_char(1), Some(' ' | '\t' | '\n')))
                {
                    break;
                }
            }
            if !key_mode
                && stop_chars.is_some()
                && ch == ':'
                && matches!(self.peek_char(1), Some(' ' | '\n' | ',' | ']' | '}'))
            {
                break;
            }
            if !key_mode
                && stop_chars.is_none()
                && ch == ':'
                && matches!(self.peek_char(1), Some(' ' | '\t'))
            {
                break;
            }
            if ch == '\t' && stop_chars.is_none() && buffer.contains(':') {
                return Err(self.error("Tabs are not allowed as YAML indentation."));
            }
            buffer.push(ch);
            self.advance();
        }
        self.flush_buffer(&mut buffer, &mut chunks);
        Ok(YamlPlainScalarNode {
            span: self.span_from(start),
            chunks,
        })
    }

    fn consume_plain_scalar_continuation(
        &mut self,
        buffer: &mut String,
        parent_indent: usize,
        requires_indented_continuation: bool,
    ) -> bool {
        let mut probe = self.index;
        let mut blank_lines = 0usize;
        while self.items.get(probe).and_then(StreamItem::char) == Some('\n') {
            probe += 1;
            let mut indent = 0usize;
            while self.items.get(probe).and_then(StreamItem::char) == Some(' ') {
                indent += 1;
                probe += 1;
            }
            match self.items.get(probe).and_then(StreamItem::char) {
                Some('\n') => blank_lines += 1,
                Some('#') | None => return false,
                Some(_)
                    if if requires_indented_continuation {
                        indent > parent_indent
                    } else {
                        indent >= parent_indent
                    } =>
                {
                    if indent == parent_indent
                        && (self.starts_sequence_item_at(probe)
                            || self.line_has_mapping_key_at(probe))
                    {
                        return false;
                    }
                    self.index = probe;
                    if blank_lines == 0 {
                        if !buffer.ends_with(' ') && !buffer.is_empty() {
                            buffer.push(' ');
                        }
                    } else {
                        for _ in 0..blank_lines {
                            buffer.push('\n');
                        }
                    }
                    return true;
                }
                _ => return false,
            }
        }
        false
    }

    fn consume_flow_plain_scalar_continuation(
        &mut self,
        buffer: &mut String,
        stop_chars: &[char],
        parent_indent: usize,
    ) -> bool {
        let mut probe = self.index;
        let mut breaks = 0usize;
        let continuation_indent;

        loop {
            if self.items.get(probe).and_then(StreamItem::char) != Some('\n') {
                return false;
            }
            probe += 1;
            breaks += 1;

            let mut indent = 0usize;
            while matches!(
                self.items.get(probe).and_then(StreamItem::char),
                Some(' ' | '\t')
            ) {
                indent += 1;
                probe += 1;
            }

            if self.items.get(probe).and_then(StreamItem::char) == Some('#') {
                return false;
            }

            if self.items.get(probe).and_then(StreamItem::char) == Some('\n') {
                continue;
            }

            continuation_indent = indent;
            break;
        }

        let Some(next_char) = self.items.get(probe).and_then(StreamItem::char) else {
            return false;
        };
        if continuation_indent < parent_indent {
            return false;
        }
        if stop_chars.contains(&next_char) {
            return false;
        }

        self.index = probe;
        if breaks == 1 {
            if !buffer.ends_with(' ') && !buffer.is_empty() {
                buffer.push(' ');
            }
        } else {
            for _ in 0..(breaks - 1) {
                buffer.push('\n');
            }
        }
        true
    }

    fn parse_block_scalar(&mut self, parent_indent: usize) -> BackendResult<YamlBlockScalarNode> {
        let start = self.mark();
        let style = self.current_char().unwrap_or('|').to_string();
        self.advance();
        let mut chomping = None;
        let mut indent_indicator = None;
        for _ in 0..2 {
            if chomping.is_none() && matches!(self.current_char(), Some('+' | '-')) {
                chomping = self.current_char().map(|ch| ch.to_string());
                self.advance();
                continue;
            }
            if indent_indicator.is_none()
                && self.current_char().is_some_and(|ch| ch.is_ascii_digit())
            {
                indent_indicator = self
                    .current_char()
                    .and_then(|ch| ch.to_digit(10))
                    .map(|value| value as usize);
                self.advance();
                continue;
            }
            break;
        }
        self.consume_line_end_required()?;
        let block_indent = if let Some(indent_indicator) = indent_indicator {
            indent_indicator
        } else {
            self.infer_block_scalar_indent(parent_indent)?
        };
        let mut chunks = Vec::new();
        let mut buffer = String::new();
        while self.current_kind() != "eof" {
            if self.line_is_blank() {
                while self.current_char() == Some(' ') {
                    self.advance();
                }
                if self.current_char() == Some('\n') {
                    buffer.push('\n');
                    self.advance();
                    continue;
                }
            }
            if self.current_line_indent() < block_indent {
                break;
            }
            self.consume_indent(block_indent)?;
            while self.current_kind() != "eof" && self.current_char() != Some('\n') {
                if self.current_kind() == "interpolation" {
                    self.flush_buffer(&mut buffer, &mut chunks);
                    chunks.push(YamlChunk::Interpolation(
                        self.consume_interpolation("string_fragment")?,
                    ));
                    continue;
                }
                if let Some(ch) = self.current_char() {
                    buffer.push(ch);
                    self.advance();
                }
            }
            if self.current_char() == Some('\n') {
                buffer.push('\n');
                self.advance();
            }
        }
        self.flush_buffer(&mut buffer, &mut chunks);
        Ok(YamlBlockScalarNode {
            span: self.span_from(start),
            style,
            chomping,
            indent_indicator,
            chunks,
        })
    }

    fn infer_block_scalar_indent(&self, parent_indent: usize) -> BackendResult<usize> {
        let mut probe = self.index;
        let mut leading_blank_indent = 0usize;
        while probe < self.items.len() {
            let mut indent = 0usize;
            while self.items.get(probe).and_then(StreamItem::char) == Some(' ') {
                indent += 1;
                probe += 1;
            }
            match self.items.get(probe).and_then(StreamItem::char) {
                Some('\n') => {
                    leading_blank_indent = leading_blank_indent.max(indent);
                    probe += 1;
                }
                Some(_) => {
                    let is_zero_indented_comment_like_content = parent_indent == 0
                        && indent == 0
                        && self.items.get(probe).and_then(StreamItem::char) == Some('#');
                    if !is_zero_indented_comment_like_content && leading_blank_indent > indent {
                        return Err(self.error("Incorrect YAML indentation."));
                    }
                    return Ok(if parent_indent == 0 {
                        indent
                    } else {
                        indent.max(parent_indent + 1)
                    });
                }
                None => break,
            }
        }
        if leading_blank_indent > parent_indent {
            return Err(self.error("Incorrect YAML indentation."));
        }
        if parent_indent == 0 {
            Ok(0)
        } else {
            Ok(parent_indent + 1)
        }
    }

    fn line_is_blank(&self) -> bool {
        let mut probe = self.index;
        while self.items.get(probe).and_then(StreamItem::char) == Some(' ') {
            probe += 1;
        }
        matches!(
            self.items.get(probe),
            Some(StreamItem::Char { ch: '\n', .. }) | Some(StreamItem::Eof { .. })
        )
    }

    fn flush_buffer(&self, buffer: &mut String, chunks: &mut Vec<YamlChunk>) {
        if buffer.is_empty() {
            return;
        }
        chunks.push(YamlChunk::Text(YamlTextChunkNode {
            span: SourceSpan::point(0, 0),
            value: std::mem::take(buffer),
        }));
    }

    fn parse_double_quoted_escape(&mut self) -> BackendResult<String> {
        self.consume_char('\\')?;
        if self.current_char() == Some('\r') {
            self.advance();
        }
        if self.current_char() == Some('\n') {
            self.advance();
            while matches!(self.current_char(), Some(' ' | '\t')) {
                self.advance();
            }
            return Ok(String::new());
        }
        let escape = self
            .current_char()
            .ok_or_else(|| self.error("Incomplete YAML escape sequence."))?;
        self.advance();
        let mapped = match escape {
            '0' => Some("\0".to_owned()),
            'a' => Some("\u{0007}".to_owned()),
            'b' => Some("\u{0008}".to_owned()),
            't' | '\t' => Some("\t".to_owned()),
            'n' => Some("\n".to_owned()),
            'v' => Some("\u{000b}".to_owned()),
            'f' => Some("\u{000c}".to_owned()),
            'r' => Some("\r".to_owned()),
            'e' => Some("\u{001b}".to_owned()),
            ' ' => Some(" ".to_owned()),
            '"' => Some("\"".to_owned()),
            '/' => Some("/".to_owned()),
            '\\' => Some("\\".to_owned()),
            'N' => Some("\u{0085}".to_owned()),
            '_' => Some("\u{00a0}".to_owned()),
            'L' => Some("\u{2028}".to_owned()),
            'P' => Some("\u{2029}".to_owned()),
            _ => None,
        };
        if let Some(value) = mapped {
            return Ok(value);
        }
        let (digits, radix_name) = match escape {
            'x' => (2, "hex"),
            'u' => (4, "unicode"),
            'U' => (8, "unicode"),
            _ => {
                return Err(self.error("Invalid YAML escape sequence."));
            }
        };
        let chars = self.collect_exact_chars(digits)?;
        let codepoint = u32::from_str_radix(&chars, 16)
            .map_err(|_| self.error(format!("Invalid YAML {radix_name} escape.")))?;
        char::from_u32(codepoint)
            .map(|value| value.to_string())
            .ok_or_else(|| self.error("Invalid YAML unicode escape."))
    }

    fn collect_exact_chars(&mut self, count: usize) -> BackendResult<String> {
        let mut chars = String::new();
        for _ in 0..count {
            let ch = self
                .current_char()
                .ok_or_else(|| self.error("Unexpected end of YAML escape sequence."))?;
            chars.push(ch);
            self.advance();
        }
        Ok(chars)
    }

    fn consume_char(&mut self, expected: char) -> BackendResult<()> {
        if self.current_char() != Some(expected) {
            return Err(self.error(format!("Expected {expected:?} in YAML template.")));
        }
        self.advance();
        Ok(())
    }

    fn consume_interpolation(&mut self, role: &str) -> BackendResult<YamlInterpolationNode> {
        let (interpolation_index, span) = match self.current() {
            StreamItem::Interpolation {
                interpolation_index,
                span,
                ..
            } => (*interpolation_index, span.clone()),
            _ => return Err(self.error("Expected an interpolation.")),
        };
        self.advance();
        Ok(YamlInterpolationNode {
            span,
            interpolation_index,
            role: role.to_owned(),
        })
    }
}

#[derive(Clone, Debug)]
struct YamlDocumentFragment {
    directives: Vec<String>,
    explicit_start: bool,
    explicit_end: bool,
    items: Vec<StreamItem>,
}

pub fn parse_template_with_profile(
    template: &TemplateInput,
    _profile: YamlProfile,
) -> BackendResult<YamlStreamNode> {
    // YAML only exposes 1.2.2 in this phase, but the parameter keeps the
    // parser wired for profile-aware dispatch once additional variants land.
    let fragments = split_stream(template)?;
    let mut documents = Vec::new();
    for fragment in fragments {
        let mut parser = YamlParser::from_items(fragment.items);
        let mut document = parser.parse()?;
        document.directives = fragment.directives;
        document.explicit_start = fragment.explicit_start;
        document.explicit_end = fragment.explicit_end;
        documents.push(document);
    }
    Ok(YamlStreamNode {
        span: documents
            .first()
            .map(|document| document.span.clone())
            .unwrap_or_else(|| SourceSpan::point(0, 0)),
        documents,
    })
}

pub fn parse_template(template: &TemplateInput) -> BackendResult<YamlStreamNode> {
    parse_template_with_profile(template, YamlProfile::default())
}

pub fn parse_validated_template_with_profile(
    template: &TemplateInput,
    profile: YamlProfile,
) -> BackendResult<YamlStreamNode> {
    let stream = parse_template_with_profile(template, profile)?;
    validate_template_stream(&stream)?;
    Ok(stream)
}

pub fn parse_validated_template(template: &TemplateInput) -> BackendResult<YamlStreamNode> {
    parse_validated_template_with_profile(template, YamlProfile::default())
}

pub fn validate_template_with_profile(
    template: &TemplateInput,
    profile: YamlProfile,
) -> BackendResult<()> {
    let stream = parse_template_with_profile(template, profile)?;
    validate_template_stream(&stream)
}

pub fn validate_template(template: &TemplateInput) -> BackendResult<()> {
    validate_template_with_profile(template, YamlProfile::default())
}

pub fn check_template_with_profile(
    template: &TemplateInput,
    profile: YamlProfile,
) -> BackendResult<()> {
    validate_template_with_profile(template, profile)
}

pub fn check_template(template: &TemplateInput) -> BackendResult<()> {
    check_template_with_profile(template, YamlProfile::default())
}

pub fn format_template_with_profile(
    template: &TemplateInput,
    profile: YamlProfile,
) -> BackendResult<String> {
    let stream = parse_validated_template_with_profile(template, profile)?;
    format_yaml_stream(template, &stream)
}

pub fn format_template(template: &TemplateInput) -> BackendResult<String> {
    format_template_with_profile(template, YamlProfile::default())
}

fn validate_template_stream(stream: &YamlStreamNode) -> BackendResult<()> {
    for document in &stream.documents {
        validate_value_node(&document.value)?;
    }
    Ok(())
}

fn validate_value_node(node: &YamlValueNode) -> BackendResult<()> {
    match node {
        YamlValueNode::Scalar(YamlScalarNode::Plain(node)) => validate_plain_scalar_node(node),
        YamlValueNode::Mapping(node) => {
            for entry in &node.entries {
                validate_key_node(&entry.key)?;
                validate_value_node(&entry.value)?;
            }
            Ok(())
        }
        YamlValueNode::Sequence(node) => {
            for item in &node.items {
                validate_value_node(item)?;
            }
            Ok(())
        }
        YamlValueNode::Decorated(node) => validate_value_node(&node.value),
        YamlValueNode::Interpolation(_)
        | YamlValueNode::Scalar(
            YamlScalarNode::DoubleQuoted(_)
            | YamlScalarNode::SingleQuoted(_)
            | YamlScalarNode::Block(_)
            | YamlScalarNode::Alias(_),
        ) => Ok(()),
    }
}

fn validate_key_node(node: &YamlKeyNode) -> BackendResult<()> {
    match &node.value {
        YamlKeyValue::Scalar(YamlScalarNode::Plain(node)) => validate_plain_scalar_node(node),
        YamlKeyValue::Complex(node) => validate_value_node(node),
        YamlKeyValue::Interpolation(_)
        | YamlKeyValue::Scalar(
            YamlScalarNode::DoubleQuoted(_)
            | YamlScalarNode::SingleQuoted(_)
            | YamlScalarNode::Block(_)
            | YamlScalarNode::Alias(_),
        ) => Ok(()),
    }
}

fn validate_plain_scalar_node(node: &YamlPlainScalarNode) -> BackendResult<()> {
    let has_interpolation = node
        .chunks
        .iter()
        .any(|chunk| matches!(chunk, YamlChunk::Interpolation(_)));
    let has_whitespace_text = node.chunks.iter().any(|chunk| {
        matches!(chunk, YamlChunk::Text(text) if text.value.chars().any(char::is_whitespace))
    });

    if has_interpolation && has_whitespace_text {
        return Err(BackendError::parse_at(
            "yaml.parse",
            "Quote YAML plain scalars that mix whitespace and interpolations.",
            Some(node.span.clone()),
        ));
    }

    Ok(())
}

pub fn normalize_documents_with_profile(
    documents: &[YamlOwned],
    _profile: YamlProfile,
) -> BackendResult<NormalizedStream> {
    // YAML normalization does not vary by profile yet, but the signature does
    // so future profile-specific semantics can plug in without reshaping APIs.
    documents
        .iter()
        .map(normalize_document)
        .collect::<BackendResult<Vec<_>>>()
        .map(NormalizedStream::new)
}

pub fn normalize_documents(documents: &[YamlOwned]) -> BackendResult<NormalizedStream> {
    normalize_documents_with_profile(documents, YamlProfile::default())
}

pub fn align_normalized_stream_with_ast(
    stream: &YamlStreamNode,
    normalized: &mut NormalizedStream,
) {
    for (document_node, document) in stream.documents.iter().zip(normalized.documents.iter_mut()) {
        align_document_with_ast(document_node, document);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CollectionRenderContext {
    BlockAllowed,
    FlowRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RenderedLayout {
    Inline,
    Block,
    Flow,
}

struct RenderedYamlValue {
    text: String,
    layout: RenderedLayout,
    empty_collection: bool,
}

impl RenderedYamlValue {
    fn inline(text: String) -> Self {
        Self {
            text,
            layout: RenderedLayout::Inline,
            empty_collection: false,
        }
    }

    fn block(text: String) -> Self {
        Self {
            text,
            layout: RenderedLayout::Block,
            empty_collection: false,
        }
    }

    fn flow(text: String, empty_collection: bool) -> Self {
        Self {
            text,
            layout: RenderedLayout::Flow,
            empty_collection,
        }
    }
}

fn format_yaml_stream(template: &TemplateInput, stream: &YamlStreamNode) -> BackendResult<String> {
    let mut parts = Vec::with_capacity(stream.documents.len());
    for document in &stream.documents {
        let mut lines = Vec::new();
        lines.extend(document.directives.iter().cloned());
        if document.explicit_start || !document.directives.is_empty() || stream.documents.len() > 1
        {
            lines.push("---".to_owned());
        }
        lines.push(
            format_yaml_value(
                template,
                &document.value,
                0,
                CollectionRenderContext::BlockAllowed,
            )?
            .text,
        );
        if document.explicit_end {
            lines.push("...".to_owned());
        }
        parts.push(lines.join("\n"));
    }
    Ok(parts.join("\n"))
}

fn format_yaml_value(
    template: &TemplateInput,
    node: &YamlValueNode,
    indent: usize,
    context: CollectionRenderContext,
) -> BackendResult<RenderedYamlValue> {
    match node {
        YamlValueNode::Decorated(node) => {
            let mut prefix = String::new();
            if let Some(tag) = &node.tag {
                prefix.push('!');
                prefix.push_str(&assemble_yaml_chunks(template, &tag.chunks, true)?);
            }
            if let Some(anchor) = &node.anchor {
                if !prefix.is_empty() {
                    prefix.push(' ');
                }
                prefix.push('&');
                prefix.push_str(&assemble_yaml_chunks(template, &anchor.chunks, true)?);
            }
            let rendered = format_yaml_value(template, node.value.as_ref(), indent, context)?;
            if prefix.is_empty() {
                Ok(rendered)
            } else {
                Ok(apply_rendered_prefix(prefix, rendered))
            }
        }
        YamlValueNode::Mapping(node) => format_yaml_mapping(template, node, indent, context),
        YamlValueNode::Sequence(node) => format_yaml_sequence(template, node, indent, context),
        YamlValueNode::Interpolation(node) => Ok(RenderedYamlValue::inline(
            interpolation_raw_source(template, node.interpolation_index, &node.span, "YAML value")?,
        )),
        YamlValueNode::Scalar(YamlScalarNode::Alias(node)) => Ok(RenderedYamlValue::inline(
            format!("*{}", assemble_yaml_chunks(template, &node.chunks, true)?),
        )),
        YamlValueNode::Scalar(YamlScalarNode::Block(node)) => Ok(RenderedYamlValue::inline(
            format_yaml_block_scalar(template, node, indent)?,
        )),
        YamlValueNode::Scalar(YamlScalarNode::DoubleQuoted(node)) => Ok(RenderedYamlValue::inline(
            serde_json::to_string(&assemble_yaml_chunks(template, &node.chunks, false)?).unwrap(),
        )),
        YamlValueNode::Scalar(YamlScalarNode::SingleQuoted(node)) => {
            Ok(RenderedYamlValue::inline(format!(
                "'{}'",
                assemble_yaml_chunks(template, &node.chunks, false)?.replace('\'', "''")
            )))
        }
        YamlValueNode::Scalar(YamlScalarNode::Plain(node)) => Ok(RenderedYamlValue::inline(
            format_yaml_plain_scalar(template, node)?,
        )),
    }
}

fn format_yaml_mapping(
    template: &TemplateInput,
    node: &YamlMappingNode,
    indent: usize,
    context: CollectionRenderContext,
) -> BackendResult<RenderedYamlValue> {
    if node.flow || context == CollectionRenderContext::FlowRequired {
        let mut entries = Vec::with_capacity(node.entries.len());
        for entry in &node.entries {
            let rendered_key = match &entry.key.value {
                YamlKeyValue::Complex(key) => {
                    format_yaml_value(template, key, indent, CollectionRenderContext::FlowRequired)?
                        .text
                }
                _ => format_yaml_key(template, &entry.key)?,
            };
            let rendered_key = normalize_flow_key_text(rendered_key);
            let rendered_value = format_yaml_value(
                template,
                &entry.value,
                indent,
                CollectionRenderContext::FlowRequired,
            )?;
            entries.push(format!("{rendered_key}: {}", rendered_value.text));
        }
        return Ok(RenderedYamlValue::flow(
            format!("{{ {} }}", entries.join(", ")),
            node.entries.is_empty(),
        ));
    }

    let mut rendered = String::new();
    for entry in &node.entries {
        if let YamlKeyValue::Complex(key) = &entry.key.value {
            let rendered_key = format_yaml_value(
                template,
                key,
                indent + 2,
                CollectionRenderContext::FlowRequired,
            )?;
            let rendered_value = format_yaml_value(
                template,
                &entry.value,
                indent + 2,
                CollectionRenderContext::BlockAllowed,
            )?;
            push_yaml_section(
                &mut rendered,
                format!("{}? {}", " ".repeat(indent), rendered_key.text),
            );
            push_rendered_value_with_prefix(
                &mut rendered,
                format!("{}:", " ".repeat(indent)),
                rendered_value,
            );
            continue;
        }
        let key = format_yaml_key(template, &entry.key)?;
        let rendered_value = format_yaml_value(
            template,
            &entry.value,
            indent + 2,
            CollectionRenderContext::BlockAllowed,
        )?;
        push_rendered_value_with_prefix(
            &mut rendered,
            format!("{}{}:", " ".repeat(indent), key),
            rendered_value,
        );
    }
    Ok(RenderedYamlValue::block(rendered))
}

fn format_yaml_sequence(
    template: &TemplateInput,
    node: &YamlSequenceNode,
    indent: usize,
    context: CollectionRenderContext,
) -> BackendResult<RenderedYamlValue> {
    if node.flow || context == CollectionRenderContext::FlowRequired {
        let items = node
            .items
            .iter()
            .map(|item| {
                format_yaml_value(
                    template,
                    item,
                    indent,
                    CollectionRenderContext::FlowRequired,
                )
                .map(|item| item.text)
            })
            .collect::<BackendResult<Vec<_>>>()?;
        return Ok(RenderedYamlValue::flow(
            format!("[ {} ]", items.join(", ")),
            node.items.is_empty(),
        ));
    }

    let mut rendered = String::new();
    for item in &node.items {
        let rendered_item = format_yaml_value(
            template,
            item,
            indent + 2,
            CollectionRenderContext::BlockAllowed,
        )?;
        push_rendered_value_with_prefix(
            &mut rendered,
            format!("{}-", " ".repeat(indent)),
            rendered_item,
        );
    }
    Ok(RenderedYamlValue::block(rendered))
}

fn format_yaml_key(template: &TemplateInput, node: &YamlKeyNode) -> BackendResult<String> {
    match &node.value {
        YamlKeyValue::Interpolation(value) => {
            interpolation_raw_source(template, value.interpolation_index, &value.span, "YAML key")
        }
        YamlKeyValue::Scalar(YamlScalarNode::DoubleQuoted(value)) => Ok(serde_json::to_string(
            &assemble_yaml_chunks(template, &value.chunks, false)?,
        )
        .unwrap()),
        YamlKeyValue::Scalar(YamlScalarNode::SingleQuoted(value)) => Ok(format!(
            "'{}'",
            assemble_yaml_chunks(template, &value.chunks, false)?.replace('\'', "''")
        )),
        YamlKeyValue::Scalar(YamlScalarNode::Plain(value)) => {
            format_yaml_plain_scalar(template, value)
        }
        YamlKeyValue::Scalar(YamlScalarNode::Alias(node)) => Ok(format!(
            "*{}",
            assemble_yaml_chunks(template, &node.chunks, true)?
        )),
        YamlKeyValue::Scalar(YamlScalarNode::Block(node)) => {
            format_yaml_block_scalar(template, node, 0)
        }
        YamlKeyValue::Complex(value) => {
            format_yaml_value(template, value, 0, CollectionRenderContext::FlowRequired)
                .map(|value| value.text)
        }
    }
}

fn format_yaml_plain_scalar(
    template: &TemplateInput,
    node: &YamlPlainScalarNode,
) -> BackendResult<String> {
    let text = assemble_yaml_chunks(template, &node.chunks, false)?
        .trim()
        .to_owned();
    if text.is_empty() {
        return Ok("null".to_owned());
    }
    if text.contains('\n') {
        return Ok(serde_json::to_string(&text).unwrap());
    }
    Ok(text)
}

fn format_yaml_block_scalar(
    template: &TemplateInput,
    node: &YamlBlockScalarNode,
    indent: usize,
) -> BackendResult<String> {
    let mut header = node.style.clone();
    if let Some(chomping) = &node.chomping {
        header.push_str(chomping);
    }
    if let Some(indent_indicator) = node.indent_indicator {
        header.push_str(&indent_indicator.to_string());
    }
    let content = assemble_yaml_chunks(template, &node.chunks, false)?;
    if content.is_empty() {
        let mut rendered = header;
        rendered.push('\n');
        return Ok(rendered);
    }

    let indentation_width = node
        .indent_indicator
        .map_or(indent, |indicator| indent.saturating_sub(2) + indicator);
    let indentation = " ".repeat(indentation_width);
    let trailing_breaks = content.chars().rev().take_while(|&ch| ch == '\n').count();
    let body_content = content.trim_end_matches('\n');
    let body = if body_content.is_empty() {
        String::new()
    } else {
        body_content
            .split('\n')
            .map(|line| format!("{indentation}{line}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let trailing = match node.chomping.as_deref() {
        Some("-") => 0,
        Some("+") => trailing_breaks.max(1),
        _ => usize::from(!content.is_empty()),
    };

    let mut rendered = header;
    rendered.push('\n');
    rendered.push_str(&body);
    for _ in 0..trailing {
        rendered.push('\n');
    }
    Ok(rendered)
}

fn assemble_yaml_chunks(
    template: &TemplateInput,
    chunks: &[YamlChunk],
    _metadata: bool,
) -> BackendResult<String> {
    let mut text = String::new();
    for chunk in chunks {
        match chunk {
            YamlChunk::Text(chunk) => text.push_str(&chunk.value),
            YamlChunk::Interpolation(chunk) => text.push_str(&interpolation_raw_source(
                template,
                chunk.interpolation_index,
                &chunk.span,
                "YAML fragment",
            )?),
        }
    }
    Ok(text)
}

fn apply_rendered_prefix(prefix: String, rendered: RenderedYamlValue) -> RenderedYamlValue {
    if rendered.layout == RenderedLayout::Block {
        return RenderedYamlValue {
            text: format!("{prefix}\n{}", rendered.text),
            layout: RenderedLayout::Block,
            empty_collection: rendered.empty_collection,
        };
    }
    RenderedYamlValue {
        text: format!("{prefix} {}", rendered.text),
        layout: rendered.layout,
        empty_collection: rendered.empty_collection,
    }
}

fn push_rendered_value_with_prefix(
    output: &mut String,
    prefix: String,
    rendered: RenderedYamlValue,
) {
    if rendered.layout == RenderedLayout::Block && !rendered.empty_collection {
        if let Some((first, rest)) = rendered.text.split_once('\n')
            && !first.starts_with(' ')
        {
            push_yaml_section(output, format!("{prefix} {first}"));
            if !rest.is_empty() {
                push_yaml_section(output, rest.to_owned());
            }
            return;
        }
        push_yaml_section(output, prefix);
        push_yaml_section(output, rendered.text);
        return;
    }

    push_yaml_section(output, format!("{prefix} {}", rendered.text));
}

fn push_yaml_section(output: &mut String, section: String) {
    if output.is_empty() {
        output.push_str(&section);
        return;
    }
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(&section);
}

fn normalize_flow_key_text(rendered_key: String) -> String {
    if rendered_key.starts_with('?') && !rendered_key.starts_with("? ") {
        return format!("? {}", &rendered_key[1..]);
    }
    rendered_key
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
                "yaml.format",
                format!(
                    "Cannot format {context} interpolation {expression:?} without raw source text."
                ),
                Some(span.clone()),
            )
        })
}

fn normalize_document(document: &YamlOwned) -> BackendResult<NormalizedDocument> {
    if matches!(document, YamlOwned::BadValue) {
        return Ok(NormalizedDocument::Empty);
    }
    Ok(NormalizedDocument::Value(normalize_value(document)?))
}

fn align_document_with_ast(node: &YamlDocumentNode, document: &mut NormalizedDocument) {
    if let NormalizedDocument::Value(value) = document {
        align_value_with_ast(&node.value, value);
    }
}

fn normalize_value(value: &YamlOwned) -> BackendResult<NormalizedValue> {
    match value {
        YamlOwned::Value(value) => normalize_scalar(value),
        YamlOwned::Representation(value, style, tag) => {
            normalize_representation_value(value, *style, tag.as_ref())
        }
        YamlOwned::Sequence(values) => values
            .iter()
            .map(normalize_value)
            .collect::<BackendResult<Vec<_>>>()
            .map(NormalizedValue::Sequence),
        YamlOwned::Mapping(values) => normalize_mapping(values),
        YamlOwned::Tagged(tag, value) => {
            normalize_tagged_value(tag.handle.as_str(), &tag.suffix, value)
        }
        YamlOwned::Alias(_) => Err(BackendError::semantic(
            "Rendered YAML still contains an unresolved alias after validation.",
        )),
        YamlOwned::BadValue => Ok(NormalizedValue::Null),
    }
}

fn normalize_representation_value(
    value: &str,
    style: ScalarStyle,
    tag: Option<&Tag>,
) -> BackendResult<NormalizedValue> {
    let parsed = ScalarOwned::parse_from_cow_and_metadata(
        Cow::Borrowed(value),
        style,
        tag.map(Cow::Borrowed).as_ref(),
    )
    .ok_or_else(|| {
        BackendError::semantic(format!(
            "Rendered YAML representation {value:?} could not be normalized with the active tag/schema."
        ))
    })?;
    normalize_scalar(&parsed)
}

fn normalize_scalar(value: &saphyr::ScalarOwned) -> BackendResult<NormalizedValue> {
    match value {
        saphyr::ScalarOwned::Null => Ok(NormalizedValue::Null),
        saphyr::ScalarOwned::Boolean(value) => Ok(NormalizedValue::Bool(*value)),
        saphyr::ScalarOwned::Integer(value) => Ok(NormalizedValue::Integer((*value).into())),
        saphyr::ScalarOwned::FloatingPoint(value) => {
            let value = value.into_inner();
            if !value.is_finite() {
                return Err(BackendError::semantic(
                    "Rendered YAML contained a non-finite float, which this backend does not normalize.",
                ));
            }
            Ok(NormalizedValue::Float(NormalizedFloat::finite(value)))
        }
        saphyr::ScalarOwned::String(value) => Ok(NormalizedValue::String(value.clone())),
    }
}

fn normalize_mapping(values: &saphyr::MappingOwned) -> BackendResult<NormalizedValue> {
    let mut entries = Vec::new();
    for (key, value) in values {
        if is_merge_key(key) {
            apply_merge_entries(value, &mut entries)?;
            continue;
        }
        insert_mapping_entry(
            &mut entries,
            normalize_key(key)?,
            normalize_value(value)?,
            true,
        );
    }
    Ok(NormalizedValue::Mapping(entries))
}

fn align_value_with_ast(node: &YamlValueNode, normalized: &mut NormalizedValue) {
    match node {
        YamlValueNode::Decorated(decorated) => {
            if decorated.tag.as_ref().is_some_and(is_set_tag_literal)
                && let NormalizedValue::Mapping(entries) = normalized
            {
                let keys = entries.iter().map(|entry| entry.key.clone()).collect();
                *normalized = NormalizedValue::Set(keys);
                return;
            }
            align_value_with_ast(&decorated.value, normalized);
        }
        YamlValueNode::Mapping(mapping) => {
            if let NormalizedValue::Mapping(entries) = normalized {
                for (entry_node, entry) in mapping.entries.iter().zip(entries.iter_mut()) {
                    align_key_with_ast(mapping.flow, &entry_node.key, &mut entry.key);
                    align_value_with_ast(&entry_node.value, &mut entry.value);
                }
            }
        }
        YamlValueNode::Sequence(sequence) => {
            if let NormalizedValue::Sequence(values) = normalized {
                for (item_node, value) in sequence.items.iter().zip(values.iter_mut()) {
                    align_value_with_ast(item_node, value);
                }
            }
        }
        YamlValueNode::Scalar(_) | YamlValueNode::Interpolation(_) => {}
    }
}

fn align_key_with_ast(flow_mapping: bool, key: &YamlKeyNode, normalized: &mut NormalizedKey) {
    match &key.value {
        YamlKeyValue::Scalar(YamlScalarNode::Plain(node))
            if flow_mapping && plain_scalar_starts_with_question(node) =>
        {
            if let NormalizedKey::String(value) = normalized
                && let Some(stripped) = value.strip_prefix('?')
            {
                *value = stripped.to_owned();
            }
        }
        YamlKeyValue::Complex(value) => align_key_value_with_ast(value, normalized),
        YamlKeyValue::Scalar(_) | YamlKeyValue::Interpolation(_) => {}
    }
}

fn align_key_value_with_ast(node: &YamlValueNode, normalized: &mut NormalizedKey) {
    match node {
        YamlValueNode::Decorated(decorated) => {
            align_key_value_with_ast(&decorated.value, normalized)
        }
        YamlValueNode::Sequence(sequence) => {
            if let NormalizedKey::Sequence(keys) = normalized {
                for (item_node, key) in sequence.items.iter().zip(keys.iter_mut()) {
                    align_key_value_with_ast(item_node, key);
                }
            }
        }
        YamlValueNode::Mapping(mapping) => {
            if let NormalizedKey::Mapping(entries) = normalized {
                for (entry_node, entry) in mapping.entries.iter().zip(entries.iter_mut()) {
                    align_key_with_ast(mapping.flow, &entry_node.key, &mut entry.key);
                    align_key_value_with_ast(&entry_node.value, &mut entry.value);
                }
            }
        }
        YamlValueNode::Scalar(_) | YamlValueNode::Interpolation(_) => {}
    }
}

fn plain_scalar_starts_with_question(node: &YamlPlainScalarNode) -> bool {
    let Some(YamlChunk::Text(text)) = node.chunks.first() else {
        return false;
    };
    text.value.starts_with('?')
}

fn is_set_tag_literal(tag: &YamlTagNode) -> bool {
    let mut literal = String::new();
    for chunk in &tag.chunks {
        let YamlChunk::Text(text) = chunk else {
            return false;
        };
        literal.push_str(&text.value);
    }
    matches!(
        literal.as_str(),
        "!set" | "!!set" | "<tag:yaml.org,2002:set>" | "tag:yaml.org,2002:set"
    )
}

fn normalize_key(value: &YamlOwned) -> BackendResult<NormalizedKey> {
    match value {
        YamlOwned::Value(value) => match value {
            saphyr::ScalarOwned::Null => Ok(NormalizedKey::Null),
            saphyr::ScalarOwned::Boolean(value) => Ok(NormalizedKey::Bool(*value)),
            saphyr::ScalarOwned::Integer(value) => Ok(NormalizedKey::Integer((*value).into())),
            saphyr::ScalarOwned::FloatingPoint(value) => {
                let value = value.into_inner();
                if !value.is_finite() {
                    return Err(BackendError::semantic(
                        "Rendered YAML contained a non-finite float mapping key, which this backend does not normalize.",
                    ));
                }
                Ok(NormalizedKey::Float(NormalizedFloat::finite(value)))
            }
            saphyr::ScalarOwned::String(value) => Ok(NormalizedKey::String(value.clone())),
        },
        YamlOwned::Representation(value, style, tag) => {
            normalize_representation_key(value, *style, tag.as_ref())
        }
        YamlOwned::Sequence(values) => values
            .iter()
            .map(normalize_key)
            .collect::<BackendResult<Vec<_>>>()
            .map(NormalizedKey::Sequence),
        YamlOwned::Mapping(values) => values
            .iter()
            .map(|(key, value)| {
                Ok(NormalizedKeyEntry {
                    key: normalize_key(key)?,
                    value: normalize_key(value)?,
                })
            })
            .collect::<BackendResult<Vec<_>>>()
            .map(NormalizedKey::Mapping),
        YamlOwned::Tagged(tag, value) => {
            normalize_tagged_key(tag.handle.as_str(), &tag.suffix, value)
        }
        YamlOwned::Alias(_) => Err(BackendError::semantic(
            "Rendered YAML still contains an unresolved alias after validation.",
        )),
        YamlOwned::BadValue => Ok(NormalizedKey::Null),
    }
}

fn normalize_representation_key(
    value: &str,
    style: ScalarStyle,
    tag: Option<&Tag>,
) -> BackendResult<NormalizedKey> {
    let parsed = ScalarOwned::parse_from_cow_and_metadata(
        Cow::Borrowed(value),
        style,
        tag.map(Cow::Borrowed).as_ref(),
    )
    .ok_or_else(|| {
        BackendError::semantic(format!(
            "Rendered YAML key representation {value:?} could not be normalized with the active tag/schema."
        ))
    })?;
    match parsed {
        ScalarOwned::Null => Ok(NormalizedKey::Null),
        ScalarOwned::Boolean(value) => Ok(NormalizedKey::Bool(value)),
        ScalarOwned::Integer(value) => Ok(NormalizedKey::Integer(value.into())),
        ScalarOwned::FloatingPoint(value) => {
            let value = value.into_inner();
            if !value.is_finite() {
                return Err(BackendError::semantic(
                    "Rendered YAML contained a non-finite float mapping key, which this backend does not normalize.",
                ));
            }
            Ok(NormalizedKey::Float(NormalizedFloat::finite(value)))
        }
        ScalarOwned::String(value) => Ok(NormalizedKey::String(value)),
    }
}

fn normalize_tagged_value(
    handle: &str,
    suffix: &str,
    value: &YamlOwned,
) -> BackendResult<NormalizedValue> {
    if is_yaml_core_tag(handle, suffix, "set") {
        let YamlOwned::Mapping(values) = value else {
            return Err(BackendError::semantic(
                "YAML !!set nodes must normalize from a mapping value.",
            ));
        };
        return values
            .iter()
            .map(|(key, _)| normalize_key(key))
            .collect::<BackendResult<Vec<_>>>()
            .map(NormalizedValue::Set);
    }
    if let Some(normalized) = normalize_core_tagged_scalar_value(handle, suffix, value)? {
        return Ok(normalized);
    }
    normalize_value(value)
}

fn normalize_tagged_key(
    handle: &str,
    suffix: &str,
    value: &YamlOwned,
) -> BackendResult<NormalizedKey> {
    if is_yaml_core_tag(handle, suffix, "set") {
        return Err(BackendError::semantic(
            "YAML set values cannot be used as mapping keys in normalized output.",
        ));
    }
    if let Some(normalized) = normalize_core_tagged_scalar_key(handle, suffix, value)? {
        return Ok(normalized);
    }
    normalize_key(value)
}

fn normalize_core_tagged_scalar_value(
    handle: &str,
    suffix: &str,
    value: &YamlOwned,
) -> BackendResult<Option<NormalizedValue>> {
    if !matches!(suffix, "null" | "bool" | "int" | "float" | "str")
        || !matches!(handle, "" | "!!" | "tag:yaml.org,2002:")
    {
        return Ok(None);
    }
    if is_empty_tagged_scalar(value) {
        return Ok(match suffix {
            "null" => Some(NormalizedValue::Null),
            "str" => Some(NormalizedValue::String(String::new())),
            _ => None,
        });
    }
    let Some(text) = scalar_text_for_core_tagged_value(value) else {
        return Ok(None);
    };
    normalize_representation_value(
        text.as_ref(),
        ScalarStyle::Plain,
        Some(&Tag {
            handle: canonical_core_tag_handle(handle).to_owned(),
            suffix: suffix.to_owned(),
        }),
    )
    .map(Some)
}

fn normalize_core_tagged_scalar_key(
    handle: &str,
    suffix: &str,
    value: &YamlOwned,
) -> BackendResult<Option<NormalizedKey>> {
    if !matches!(suffix, "null" | "bool" | "int" | "float" | "str")
        || !matches!(handle, "" | "!!" | "tag:yaml.org,2002:")
    {
        return Ok(None);
    }
    if is_empty_tagged_scalar(value) {
        return Ok(match suffix {
            "null" => Some(NormalizedKey::Null),
            "str" => Some(NormalizedKey::String(String::new())),
            _ => None,
        });
    }
    let Some(text) = scalar_text_for_core_tagged_value(value) else {
        return Ok(None);
    };
    normalize_representation_key(
        text.as_ref(),
        ScalarStyle::Plain,
        Some(&Tag {
            handle: canonical_core_tag_handle(handle).to_owned(),
            suffix: suffix.to_owned(),
        }),
    )
    .map(Some)
}

fn canonical_core_tag_handle(handle: &str) -> &str {
    match handle {
        "" | "!!" | "tag:yaml.org,2002:" => "tag:yaml.org,2002:",
        other => other,
    }
}

fn scalar_text_for_tagged_value(value: &YamlOwned) -> Option<Cow<'_, str>> {
    match value {
        YamlOwned::Representation(value, _, _) => Some(Cow::Borrowed(value.as_str())),
        YamlOwned::Value(ScalarOwned::Null) => Some(Cow::Borrowed("null")),
        YamlOwned::Value(ScalarOwned::Boolean(value)) => {
            Some(Cow::Borrowed(if *value { "true" } else { "false" }))
        }
        YamlOwned::Value(ScalarOwned::Integer(value)) => Some(Cow::Owned(value.to_string())),
        YamlOwned::Value(ScalarOwned::FloatingPoint(value)) => {
            Some(Cow::Owned(value.into_inner().to_string()))
        }
        YamlOwned::Value(ScalarOwned::String(value)) => Some(Cow::Borrowed(value.as_str())),
        YamlOwned::Tagged(_, value) => scalar_text_for_tagged_value(value),
        YamlOwned::Sequence(_)
        | YamlOwned::Mapping(_)
        | YamlOwned::Alias(_)
        | YamlOwned::BadValue => None,
    }
}

fn scalar_text_for_core_tagged_value(value: &YamlOwned) -> Option<Cow<'_, str>> {
    match value {
        YamlOwned::Value(ScalarOwned::Null) => Some(Cow::Borrowed("")),
        YamlOwned::Tagged(_, value) => scalar_text_for_core_tagged_value(value),
        other => scalar_text_for_tagged_value(other),
    }
}

fn is_empty_tagged_scalar(value: &YamlOwned) -> bool {
    match value {
        YamlOwned::Value(ScalarOwned::Null) => true,
        YamlOwned::Representation(value, _, _) => value.is_empty(),
        YamlOwned::Tagged(_, value) => is_empty_tagged_scalar(value),
        _ => false,
    }
}

fn is_yaml_core_tag(handle: &str, suffix: &str, expected_suffix: &str) -> bool {
    suffix == expected_suffix && matches!(handle, "" | "!!" | "tag:yaml.org,2002:")
}

fn is_merge_key(value: &YamlOwned) -> bool {
    match value {
        YamlOwned::Value(saphyr::ScalarOwned::String(value)) => value == "<<",
        YamlOwned::Representation(value, _, _) => value == "<<",
        YamlOwned::Tagged(_, value) => is_merge_key(value),
        _ => false,
    }
}

fn apply_merge_entries(value: &YamlOwned, entries: &mut Vec<NormalizedEntry>) -> BackendResult<()> {
    match value {
        YamlOwned::Mapping(values) => merge_mapping_entries(values, entries),
        YamlOwned::Sequence(values) => {
            for value in values {
                let YamlOwned::Mapping(values) = value else {
                    return Err(BackendError::semantic(
                        "YAML merge sequences must contain only mappings.",
                    ));
                };
                merge_mapping_entries(values, entries)?;
            }
            Ok(())
        }
        YamlOwned::Tagged(_, value) => apply_merge_entries(value, entries),
        _ => Err(BackendError::semantic(
            "YAML merge values must be a mapping or sequence of mappings.",
        )),
    }
}

fn merge_mapping_entries(
    values: &saphyr::MappingOwned,
    entries: &mut Vec<NormalizedEntry>,
) -> BackendResult<()> {
    for (key, value) in values {
        insert_mapping_entry(entries, normalize_key(key)?, normalize_value(value)?, false);
    }
    Ok(())
}

fn insert_mapping_entry(
    entries: &mut Vec<NormalizedEntry>,
    key: NormalizedKey,
    value: NormalizedValue,
    override_existing: bool,
) {
    if let Some(existing) = entries.iter_mut().find(|entry| entry.key == key) {
        if override_existing {
            existing.value = value;
        }
        return;
    }
    entries.push(NormalizedEntry { key, value });
}

fn wrap_decorators(
    decorated: Option<(Option<YamlTagNode>, Option<YamlAnchorNode>)>,
    value: YamlValueNode,
) -> BackendResult<YamlValueNode> {
    if let Some((tag, anchor)) = decorated {
        let decorator_span = tag
            .as_ref()
            .map(|tag| tag.span.clone())
            .or_else(|| anchor.as_ref().map(|anchor| anchor.span.clone()))
            .unwrap_or_else(|| value_span(&value).clone());
        let span = decorator_span.merge(value_span(&value));

        match value {
            YamlValueNode::Decorated(inner) => {
                if anchor.is_some() && inner.anchor.is_some() {
                    return Err(BackendError::parse_at(
                        "yaml.parse",
                        "YAML nodes cannot define more than one anchor.",
                        Some(span),
                    ));
                }
                if tag.is_some() && inner.tag.is_some() {
                    return Err(BackendError::parse_at(
                        "yaml.parse",
                        "YAML nodes cannot define more than one tag.",
                        Some(span),
                    ));
                }

                Ok(YamlValueNode::Decorated(YamlDecoratedNode {
                    span,
                    value: inner.value,
                    tag: tag.or(inner.tag),
                    anchor: anchor.or(inner.anchor),
                }))
            }
            YamlValueNode::Scalar(YamlScalarNode::Alias(_))
                if tag.is_some() || anchor.is_some() =>
            {
                Err(BackendError::parse_at(
                    "yaml.parse",
                    "YAML aliases cannot define tags or anchors.",
                    Some(span),
                ))
            }
            value => Ok(YamlValueNode::Decorated(YamlDecoratedNode {
                span,
                value: Box::new(value),
                tag,
                anchor,
            })),
        }
    } else {
        Ok(value)
    }
}

fn null_plain_scalar() -> YamlPlainScalarNode {
    YamlPlainScalarNode {
        span: SourceSpan::point(0, 0),
        chunks: vec![YamlChunk::Text(YamlTextChunkNode {
            span: SourceSpan::point(0, 0),
            value: "null".to_owned(),
        })],
    }
}

fn empty_plain_scalar() -> YamlPlainScalarNode {
    YamlPlainScalarNode {
        span: SourceSpan::point(0, 0),
        chunks: Vec::new(),
    }
}

fn flow_implicit_plain_key_needs_separator(key: &YamlKeyNode) -> bool {
    matches!(
        &key.value,
        YamlKeyValue::Scalar(YamlScalarNode::Plain(node))
            if node.chunks.iter().any(|chunk| match chunk {
                YamlChunk::Text(text) => text.value.chars().any(char::is_whitespace),
                YamlChunk::Interpolation(_) => true,
            })
    )
}

fn split_stream(template: &TemplateInput) -> BackendResult<Vec<YamlDocumentFragment>> {
    let items = template.flatten();
    let mut fragments = Vec::new();
    let mut directives = Vec::new();
    let mut current_start = None;
    let mut explicit_start = false;
    let mut explicit_end = false;
    let mut line_start = 0;

    while line_start < items.len() {
        let (line_end, next_line) = line_bounds(&items, line_start);
        let line = collect_line(&items[line_start..line_end]);
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let document_start_payload = document_start_payload_start(&items, line_start, line_end);
        let document_end = is_document_end_marker(trimmed);
        let malformed_document_end = trimmed.starts_with("...") && !document_end;

        if malformed_document_end {
            let span = items
                .get(line_start)
                .map(|item| item.span().clone())
                .unwrap_or_else(|| SourceSpan::point(0, 0));
            return Err(BackendError::parse_at(
                "yaml.parse",
                "Unexpected trailing YAML content.",
                Some(span),
            ));
        }

        if current_start.is_none() {
            if trimmed.is_empty() || trimmed.starts_with('#') {
                line_start = next_line;
                continue;
            }
            if trimmed.starts_with('%') {
                if !is_valid_directive(trimmed) {
                    let span = items
                        .get(line_start)
                        .map(|item| item.span().clone())
                        .unwrap_or_else(|| SourceSpan::point(0, 0));
                    return Err(BackendError::parse_at(
                        "yaml.parse",
                        "Unexpected trailing YAML content.",
                        Some(span),
                    ));
                }
                if duplicates_existing_directive(&directives, trimmed) {
                    let span = items
                        .get(line_start)
                        .map(|item| item.span().clone())
                        .unwrap_or_else(|| SourceSpan::point(0, 0));
                    return Err(BackendError::parse_at(
                        "yaml.parse",
                        "Unexpected trailing YAML content.",
                        Some(span),
                    ));
                }
                directives.push(trimmed.to_owned());
                line_start = next_line;
                continue;
            }
            if trimmed == "---" {
                current_start = Some(next_line);
                explicit_start = true;
                line_start = next_line;
                continue;
            }
            if let Some(payload_start) = document_start_payload {
                if explicit_start_payload_looks_like_block_mapping(trimmed) {
                    let span = items
                        .get(line_start)
                        .map(|item| item.span().clone())
                        .unwrap_or_else(|| SourceSpan::point(0, 0));
                    return Err(BackendError::parse_at(
                        "yaml.parse",
                        "Unexpected trailing YAML content.",
                        Some(span),
                    ));
                }
                current_start = Some(payload_start.unwrap_or(next_line));
                explicit_start = true;
                line_start = next_line;
                continue;
            }
            if document_end {
                line_start = next_line;
                continue;
            }
            current_start = Some(line_start);
        } else if trimmed == "---" {
            fragments.push(build_fragment(
                &items,
                current_start.unwrap_or(line_start),
                line_start,
                std::mem::take(&mut directives),
                explicit_start,
                explicit_end,
            ));
            current_start = Some(next_line);
            explicit_start = true;
            explicit_end = false;
            line_start = next_line;
            continue;
        } else if let Some(payload_start) = document_start_payload {
            fragments.push(build_fragment(
                &items,
                current_start.unwrap_or(line_start),
                line_start,
                std::mem::take(&mut directives),
                explicit_start,
                explicit_end,
            ));
            current_start = Some(payload_start.unwrap_or(next_line));
            explicit_start = true;
            explicit_end = false;
            line_start = next_line;
            continue;
        } else if document_end {
            fragments.push(build_fragment(
                &items,
                current_start.unwrap_or(line_start),
                line_start,
                std::mem::take(&mut directives),
                explicit_start,
                true,
            ));
            current_start = None;
            explicit_start = false;
            explicit_end = false;
            line_start = next_line;
            continue;
        }

        line_start = next_line;
    }

    if let Some(start) = current_start {
        fragments.push(build_fragment(
            &items,
            start,
            items.len().saturating_sub(1),
            std::mem::take(&mut directives),
            explicit_start,
            explicit_end,
        ));
    }

    if current_start.is_none() && !directives.is_empty() {
        let span = items
            .iter()
            .rev()
            .find(|item| item.kind() != "eof")
            .map(|item| item.span().clone())
            .unwrap_or_else(|| SourceSpan::point(0, 0));
        return Err(BackendError::parse_at(
            "yaml.parse",
            "Unexpected trailing YAML content.",
            Some(span),
        ));
    }

    if fragments.is_empty() {
        fragments.push(YamlDocumentFragment {
            directives: Vec::new(),
            explicit_start: false,
            explicit_end: false,
            items: vec![StreamItem::Eof {
                span: SourceSpan::point(0, 0),
            }],
        });
    }

    Ok(fragments)
}

fn build_fragment(
    items: &[StreamItem],
    start: usize,
    end: usize,
    directives: Vec<String>,
    explicit_start: bool,
    explicit_end: bool,
) -> YamlDocumentFragment {
    let mut fragment_items = items[start..end].to_vec();
    let eof_span = fragment_items
        .last()
        .map_or_else(|| SourceSpan::point(0, 0), |item| item.span().clone());
    fragment_items.push(StreamItem::Eof { span: eof_span });
    YamlDocumentFragment {
        directives,
        explicit_start,
        explicit_end,
        items: fragment_items,
    }
}

fn line_bounds(items: &[StreamItem], start: usize) -> (usize, usize) {
    let mut probe = start;
    while probe < items.len() {
        if items[probe].char() == Some('\n') {
            return (probe, probe + 1);
        }
        if items[probe].kind() == "eof" {
            return (probe, probe + 1);
        }
        probe += 1;
    }
    (items.len(), items.len())
}

fn document_start_payload_start(
    items: &[StreamItem],
    line_start: usize,
    line_end: usize,
) -> Option<Option<usize>> {
    let mut probe = line_start;
    for expected in ['-', '-', '-'] {
        if items.get(probe).and_then(StreamItem::char) != Some(expected) {
            return None;
        }
        probe += 1;
    }
    if probe >= line_end {
        return Some(None);
    }
    if !matches!(
        items.get(probe).and_then(StreamItem::char),
        Some(' ' | '\t')
    ) {
        return None;
    }
    while probe < line_end
        && matches!(
            items.get(probe).and_then(StreamItem::char),
            Some(' ' | '\t')
        )
    {
        probe += 1;
    }
    if probe >= line_end || items.get(probe).and_then(StreamItem::char) == Some('#') {
        return Some(None);
    }
    Some(Some(probe))
}

fn is_document_end_marker(trimmed: &str) -> bool {
    if !trimmed.starts_with("...") {
        return false;
    }
    let Some(remainder) = trimmed.get(3..) else {
        return true;
    };
    let remainder = remainder.trim_start_matches([' ', '\t']);
    remainder.is_empty() || remainder.starts_with('#')
}

fn explicit_start_payload_looks_like_block_mapping(trimmed: &str) -> bool {
    let Some(rest) = trimmed.strip_prefix("---") else {
        return false;
    };
    let rest = rest.trim_start_matches([' ', '\t']);
    if !(rest.starts_with('&') || rest.starts_with('!')) {
        return false;
    }
    if rest.starts_with('[') || rest.starts_with('{') {
        return false;
    }
    let Some((before_colon, after_colon)) = rest.split_once(':') else {
        return false;
    };
    !before_colon.contains('[')
        && !before_colon.contains('{')
        && !before_colon.ends_with(':')
        && matches!(after_colon.chars().next(), Some(' ' | '\t') | None)
}

fn is_valid_directive(trimmed: &str) -> bool {
    let directive = trimmed
        .split_once('#')
        .map(|(directive, _)| directive)
        .unwrap_or(trimmed)
        .trim_end();
    let mut parts = directive.split_whitespace();
    match parts.next() {
        Some("%YAML") => matches!(
            (parts.next(), parts.next(), parts.next()),
            (Some(_version), None, None)
        ),
        Some("%TAG") => matches!(
            (parts.next(), parts.next(), parts.next(), parts.next()),
            (Some(_handle), Some(_prefix), None, None)
        ),
        Some(_) => true,
        None => false,
    }
}

fn duplicates_existing_directive(existing: &[String], candidate: &str) -> bool {
    let Some((name, handle)) = directive_identity(candidate) else {
        return false;
    };
    existing.iter().any(|directive| {
        directive_identity(directive).is_some_and(|(existing_name, existing_handle)| {
            existing_name == name && existing_handle == handle
        })
    })
}

fn directive_identity(trimmed: &str) -> Option<(&str, Option<&str>)> {
    let directive = trimmed
        .split_once('#')
        .map(|(directive, _)| directive)
        .unwrap_or(trimmed)
        .trim_end();
    let mut parts = directive.split_whitespace();
    match parts.next() {
        Some("%YAML") => Some(("%YAML", None)),
        Some("%TAG") => parts.next().map(|handle| ("%TAG", Some(handle))),
        _ => None,
    }
}

fn collect_line(items: &[StreamItem]) -> String {
    let mut line = String::new();
    for item in items {
        if let Some(ch) = item.char() {
            line.push(ch);
        } else {
            line.push('\u{fffc}');
        }
    }
    line
}

fn value_span(value: &YamlValueNode) -> &SourceSpan {
    match value {
        YamlValueNode::Scalar(YamlScalarNode::Plain(node)) => &node.span,
        YamlValueNode::Scalar(YamlScalarNode::DoubleQuoted(node)) => &node.span,
        YamlValueNode::Scalar(YamlScalarNode::SingleQuoted(node)) => &node.span,
        YamlValueNode::Scalar(YamlScalarNode::Block(node)) => &node.span,
        YamlValueNode::Scalar(YamlScalarNode::Alias(node)) => &node.span,
        YamlValueNode::Interpolation(node) => &node.span,
        YamlValueNode::Mapping(node) => &node.span,
        YamlValueNode::Sequence(node) => &node.span,
        YamlValueNode::Decorated(node) => &node.span,
    }
}

#[cfg(test)]
mod tests {
    use super::{YamlValueNode, parse_template};
    use pyo3::prelude::*;
    use saphyr::{LoadableYamlNode, ScalarOwned, YamlOwned};
    use tstring_pyo3_bindings::{extract_template, yaml::render_document};
    use tstring_syntax::{BackendError, BackendResult, ErrorKind};

    fn parse_rendered_yaml(text: &str) -> BackendResult<Vec<YamlOwned>> {
        YamlOwned::load_from_str(text).map_err(|err| {
            BackendError::parse(format!(
                "Rendered YAML could not be reparsed during test verification: {err}"
            ))
        })
    }

    fn yaml_scalar_text(value: &YamlOwned) -> Option<&str> {
        match value {
            YamlOwned::Value(value) => value.as_str(),
            YamlOwned::Representation(value, _, _) => Some(value.as_str()),
            YamlOwned::Tagged(_, value) => yaml_scalar_text(value),
            _ => None,
        }
    }

    fn yaml_integer(value: &YamlOwned) -> Option<i64> {
        match value {
            YamlOwned::Value(value) => value.as_integer(),
            YamlOwned::Tagged(_, value) => yaml_integer(value),
            _ => None,
        }
    }

    fn yaml_float(value: &YamlOwned) -> Option<f64> {
        match value {
            YamlOwned::Value(ScalarOwned::FloatingPoint(value)) => Some(value.into_inner()),
            YamlOwned::Tagged(_, value) => yaml_float(value),
            _ => None,
        }
    }

    fn yaml_sequence_len(value: &YamlOwned) -> Option<usize> {
        match value {
            YamlOwned::Sequence(value) => Some(value.len()),
            YamlOwned::Tagged(_, value) => yaml_sequence_len(value),
            _ => None,
        }
    }

    fn yaml_mapping(value: &YamlOwned) -> Option<&saphyr::MappingOwned> {
        match value {
            YamlOwned::Mapping(value) => Some(value),
            YamlOwned::Tagged(_, value) => yaml_mapping(value),
            _ => None,
        }
    }

    fn assert_string_entry(document: &YamlOwned, key: &str, expected: &str) {
        let mapping = yaml_mapping(document).expect("expected YAML mapping");
        let value = mapping
            .iter()
            .find_map(|(entry_key, entry_value)| {
                (yaml_scalar_text(entry_key) == Some(key)).then_some(entry_value)
            })
            .expect("expected YAML mapping entry");
        assert_eq!(yaml_scalar_text(value), Some(expected));
    }

    fn assert_integer_entry(document: &YamlOwned, key: &str, expected: i64) {
        let mapping = yaml_mapping(document).expect("expected YAML mapping");
        let value = mapping
            .iter()
            .find_map(|(entry_key, entry_value)| {
                (yaml_scalar_text(entry_key) == Some(key)).then_some(entry_value)
            })
            .expect("expected YAML mapping entry");
        assert_eq!(yaml_integer(value), Some(expected));
    }

    fn yaml_mapping_entry<'a>(document: &'a YamlOwned, key: &str) -> Option<&'a YamlOwned> {
        yaml_mapping(document).and_then(|mapping| {
            mapping.iter().find_map(|(entry_key, entry_value)| {
                (yaml_scalar_text(entry_key) == Some(key)).then_some(entry_value)
            })
        })
    }

    #[test]
    fn parses_yaml_flow_and_scalar_nodes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "user='Alice'\ntemplate=t'name: \"hi-{user}\"\\nitems: [1, {user}]'\n"
                ),
                pyo3::ffi::c_str!("test_yaml.py"),
                pyo3::ffi::c_str!("test_yaml"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
            let stream = parse_template(&template).unwrap();
            let YamlValueNode::Mapping(mapping) = &stream.documents[0].value else {
                panic!("expected mapping");
            };
            assert_eq!(mapping.entries.len(), 2);
        });
    }

    #[test]
    fn parses_tags_and_anchors_on_scalars() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "tag='str'\nanchor='user'\ntemplate=t'value: !{tag} &{anchor} \"hi\"\\n'\n"
                ),
                pyo3::ffi::c_str!("test_yaml_decorators.py"),
                pyo3::ffi::c_str!("test_yaml_decorators"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
            let stream = parse_template(&template).unwrap();
            let YamlValueNode::Mapping(mapping) = &stream.documents[0].value else {
                panic!("expected mapping");
            };
            let YamlValueNode::Decorated(node) = &mapping.entries[0].value else {
                panic!("expected decorated scalar value");
            };
            assert!(node.tag.is_some());
            assert!(node.anchor.is_some());
        });
    }

    #[test]
    fn renders_nested_yaml_values_and_validates() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "name='Ada'\nmeta={'active': True, 'count': 2}\ntemplate=t'name: {name}\\nmeta: {meta}\\n'\n"
                ),
                pyo3::ffi::c_str!("test_yaml_render.py"),
                pyo3::ffi::c_str!("test_yaml_render"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
            let stream = parse_template(&template).unwrap();
            let rendered = render_document(py, &stream).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();

            assert!(rendered.text.contains("name: \"Ada\""));
            assert_string_entry(&documents[0], "name", "Ada");
        });
    }

    #[test]
    fn rejects_metadata_with_whitespace() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("anchor='bad anchor'\ntemplate=t'value: &{anchor} \"hi\"\\n'\n"),
                pyo3::ffi::c_str!("test_yaml_error.py"),
                pyo3::ffi::c_str!("test_yaml_error"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
            let stream = parse_template(&template).unwrap();
            let err = match render_document(py, &stream) {
                Ok(_) => panic!("expected YAML render failure"),
                Err(err) => err,
            };

            assert_eq!(err.kind, ErrorKind::Unrepresentable);
            assert!(err.message.contains("YAML metadata"));
        });
    }

    #[test]
    fn rejects_flow_mappings_missing_commas_during_parse() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nmapping=Template('{a: 1 b: 2}\\n')\nmissing_colon=Template('value: {a b}\\n')\nleading_comma_mapping=Template('value: {, a: 1}\\n')\nsequence=Template('[1,,2]\\n')\ntrailing_sequence=Template('value: [1, 2,,]\\n')\nempty_sequence=Template('[,]\\n')\nempty_mapping=Template('value: {,}\\n')\n"
                ),
                pyo3::ffi::c_str!("test_yaml_flow_error.py"),
                pyo3::ffi::c_str!("test_yaml_flow_error"),
            )
            .unwrap();
            for name in [
                "mapping",
                "missing_colon",
                "leading_comma_mapping",
                "sequence",
                "trailing_sequence",
                "empty_sequence",
                "empty_mapping",
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
                let err = parse_template(&template).expect_err("expected YAML parse failure");
                assert_eq!(err.kind, ErrorKind::Parse);
                assert!(err.message.contains("Expected"));
            }
        });
    }

    #[test]
    fn rejects_tabs_as_mapping_separation_whitespace() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nmapping=Template('a:\\t1\\n')\nplain=Template('url: a:b\\t\\n')\nindent=Template('a:\\n\\t- 1\\n')\n"
                ),
                pyo3::ffi::c_str!("test_yaml_tab_error.py"),
                pyo3::ffi::c_str!("test_yaml_tab_error"),
            )
            .unwrap();
            for name in ["mapping", "plain", "indent"] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
                let err = parse_template(&template).expect_err("expected YAML parse failure");
                assert_eq!(err.kind, ErrorKind::Parse);
                assert!(
                    err.message.contains("Tabs are not allowed"),
                    "{name}: {}",
                    err.message
                );
            }
        });
    }

    #[test]
    fn splits_explicit_document_streams() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("template=t'---\\nname: Alice\\n---\\nname: Bob\\n'\n"),
                pyo3::ffi::c_str!("test_yaml_stream.py"),
                pyo3::ffi::c_str!("test_yaml_stream"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
            let stream = parse_template(&template).unwrap();
            assert_eq!(stream.documents.len(), 2);
        });
    }

    #[test]
    fn parses_and_renders_flow_complex_keys_with_interpolation() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "left='Alice'\nright='Bob'\ntemplate=t'{{ {{name: [{left}, {right}]}}: 1, [{left}, {right}]: 2 }}'\n"
                ),
                pyo3::ffi::c_str!("test_yaml_flow_complex_keys.py"),
                pyo3::ffi::c_str!("test_yaml_flow_complex_keys"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
            let stream = parse_template(&template).unwrap();
            let YamlValueNode::Mapping(mapping) = &stream.documents[0].value else {
                panic!("expected mapping");
            };
            assert!(matches!(
                mapping.entries[0].key.value,
                super::YamlKeyValue::Complex(_)
            ));
            assert!(matches!(
                mapping.entries[1].key.value,
                super::YamlKeyValue::Complex(_)
            ));

            let rendered = render_document(py, &stream).unwrap();
            assert_eq!(
                rendered.text,
                "{ { name: [ \"Alice\", \"Bob\" ] }: 1, [ \"Alice\", \"Bob\" ]: 2 }"
            );
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(
                documents[0]
                    .as_mapping()
                    .expect("expected YAML mapping")
                    .len(),
                2
            );
        });
    }

    #[test]
    fn parses_explicit_mapping_keys_with_nested_collections() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "left='Alice'\nright='Bob'\ntemplate=t'? {{name: [{left}, {right}]}}\\n: 1\\n'\n"
                ),
                pyo3::ffi::c_str!("test_yaml_explicit_complex_key.py"),
                pyo3::ffi::c_str!("test_yaml_explicit_complex_key"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
            let stream = parse_template(&template).unwrap();
            let YamlValueNode::Mapping(mapping) = &stream.documents[0].value else {
                panic!("expected mapping");
            };
            assert!(matches!(
                mapping.entries[0].key.value,
                super::YamlKeyValue::Complex(_)
            ));
            let rendered = render_document(py, &stream).unwrap();
            assert!(rendered.text.contains("? { name: [ \"Alice\", \"Bob\" ] }"));
        });
    }

    #[test]
    fn renders_explicit_complex_keys_text_and_validated_shape() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "left='Alice'\nright='Bob'\ncomplex_key=t'? {{name: [{left}, {right}]}}\\n: 1\\n'\nempty_key=t'?\\n: 1\\n'\n"
                ),
                pyo3::ffi::c_str!("test_yaml_explicit_key_render.py"),
                pyo3::ffi::c_str!("test_yaml_explicit_key_render"),
            )
            .unwrap();

            let complex_key = module.getattr("complex_key").unwrap();
            let complex_key = extract_template(py, &complex_key, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&complex_key).unwrap()).unwrap();
            assert!(
                rendered
                    .text
                    .contains("? { name: [ \"Alice\", \"Bob\" ] }\n: 1")
            );
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(
                documents[0]
                    .as_mapping()
                    .expect("expected YAML mapping")
                    .len(),
                1
            );

            let empty_key = module.getattr("empty_key").unwrap();
            let empty_key = extract_template(py, &empty_key, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&empty_key).unwrap()).unwrap();
            assert_eq!(rendered.text, "? null\n: 1");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(
                documents[0]
                    .as_mapping()
                    .expect("expected YAML mapping")
                    .len(),
                1
            );
        });
    }

    #[test]
    fn parses_yaml_quoted_scalar_escapes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ntemplate=t'value: \"line\\\\nnext \\\\u03B1 \\\\x41\"\\nquote: \\'it\\'\\'s ok\\''\nunicode=t'value: \"\\\\U0001D11E\"\\n'\ncrlf_join=Template('value: \"a\\\\\\r\\n  b\"\\n')\nnel=t'value: \"\\\\N\"\\n'\nnbsp=t'value: \"\\\\_\"\\n'\nempty_single=Template(\"value: ''\\n\")\nempty_double=t'value: \"\"\\n'\nsingle_blank=Template(\"value: 'a\\n\\n  b\\n\\n  c'\\n\")\n"
                ),
                pyo3::ffi::c_str!("test_yaml_quoted_scalars.py"),
                pyo3::ffi::c_str!("test_yaml_quoted_scalars"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
            let stream = parse_template(&template).unwrap();
            let rendered = render_document(py, &stream).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_string_entry(&documents[0], "value", "line\nnext α A");
            assert_string_entry(&documents[0], "quote", "it's ok");

            for (name, expected) in [
                ("unicode", "𝄞"),
                ("crlf_join", "ab"),
                ("nel", "\u{0085}"),
                ("nbsp", "\u{00a0}"),
                ("empty_single", ""),
                ("empty_double", ""),
                ("single_blank", "a\nb\nc"),
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
                let stream = parse_template(&template).unwrap();
                let rendered = render_document(py, &stream).unwrap();
                let documents = parse_rendered_yaml(&rendered.text).unwrap();
                assert_string_entry(&documents[0], "value", expected);
            }
        });
    }

    #[test]
    fn renders_quoted_scalar_escape_and_folding_families() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nbase=t'value: \"line\\\\nnext \\\\u03B1 \\\\x41\"\\nquote: \\'it\\'\\'s ok\\''\nmultiline_double=t'value: \"a\\n  b\"\\n'\nmultiline_double_blank=t'value: \"a\\n\\n  b\"\\n'\nmultiline_double_more_blank=t'value: \"a\\n\\n\\n  b\"\\n'\nunicode=t'value: \"\\\\U0001D11E\"\\n'\ncrlf_join=Template('value: \"a\\\\\\r\\n  b\"\\n')\nnel=t'value: \"\\\\N\"\\n'\nnbsp=t'value: \"\\\\_\"\\n'\nspace=t'value: \"\\\\ \"\\n'\nslash=t'value: \"\\\\/\"\\n'\ntab=t'value: \"\\\\t\"\\n'\nempty_single=Template(\"value: ''\\n\")\nempty_double=t'value: \"\"\\n'\nsingle_blank=Template(\"value: 'a\\n\\n  b\\n\\n  c'\\n\")\n"
                ),
                pyo3::ffi::c_str!("test_yaml_quoted_scalar_families.py"),
                pyo3::ffi::c_str!("test_yaml_quoted_scalar_families"),
            )
            .unwrap();

            let base = module.getattr("base").unwrap();
            let base = extract_template(py, &base, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&base).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(
                rendered.text,
                "value: \"line\\nnext α A\"\nquote: 'it''s ok'"
            );
            assert_string_entry(&documents[0], "value", "line\nnext α A");
            assert_string_entry(&documents[0], "quote", "it's ok");

            for (name, expected_text, expected) in [
                ("multiline_double", "value: \"a b\"", "a b"),
                ("multiline_double_blank", "value: \"a\\nb\"", "a\nb"),
                (
                    "multiline_double_more_blank",
                    "value: \"a\\n\\nb\"",
                    "a\n\nb",
                ),
                ("unicode", "value: \"𝄞\"", "𝄞"),
                ("crlf_join", "value: \"ab\"", "ab"),
                ("nel", "value: \"\u{0085}\"", "\u{0085}"),
                ("nbsp", "value: \"\u{00a0}\"", "\u{00a0}"),
                ("space", "value: \" \"", " "),
                ("slash", "value: \"/\"", "/"),
                ("tab", "value: \"\\t\"", "\t"),
                ("empty_single", "value: ''", ""),
                ("empty_double", "value: \"\"", ""),
                ("single_blank", "value: 'a\n\n  b\n\n  c'", "a\nb\nc"),
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
                let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
                assert_eq!(rendered.text, expected_text, "{name}");
                let documents = parse_rendered_yaml(&rendered.text).unwrap();
                assert_string_entry(&documents[0], "value", expected);
            }
        });
    }

    #[test]
    fn renders_spec_quoted_scalar_examples_round_trip() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "unicode=t'unicode: \"Sosa did fine.\\\\u263A\"'\ncontrol=t'control: \"\\\\b1998\\\\t1999\\\\t2000\\\\n\"'\nsingle=t'''single: '\"Howdy!\" he cried.' '''\nquoted=t'''quoted: ' # Not a ''comment''.' '''\ntie=t'''tie: '|\\\\-*-/|' '''\n"
                ),
                pyo3::ffi::c_str!("test_yaml_spec_quoted_examples.py"),
                pyo3::ffi::c_str!("test_yaml_spec_quoted_examples"),
            )
            .unwrap();

            for (name, key, expected_text, expected_value) in [
                (
                    "unicode",
                    "unicode",
                    "unicode: \"Sosa did fine.☺\"",
                    "Sosa did fine.\u{263a}",
                ),
                (
                    "control",
                    "control",
                    "control: \"\\b1998\\t1999\\t2000\\n\"",
                    "\u{0008}1998\t1999\t2000\n",
                ),
                (
                    "single",
                    "single",
                    "single: '\"Howdy!\" he cried.'",
                    "\"Howdy!\" he cried.",
                ),
                (
                    "quoted",
                    "quoted",
                    "quoted: ' # Not a ''comment''.'",
                    " # Not a 'comment'.",
                ),
                ("tie", "tie", "tie: '|\\-*-/|'", "|\\-*-/|"),
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
                let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
                assert_eq!(rendered.text, expected_text, "{name}");
                let documents = parse_rendered_yaml(&rendered.text).unwrap();
                assert_string_entry(&documents[0], key, expected_value);
            }
        });
    }

    #[test]
    fn folds_multiline_double_quoted_scalars() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "single=t'value: \"a\\n  b\"\\n'\nblank=t'value: \"a\\n\\n  b\"\\n'\n"
                ),
                pyo3::ffi::c_str!("test_yaml_multiline_double_quoted.py"),
                pyo3::ffi::c_str!("test_yaml_multiline_double_quoted"),
            )
            .unwrap();

            for (name, expected) in [("single", "a b"), ("blank", "a\nb")] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
                let stream = parse_template(&template).unwrap();
                let rendered = render_document(py, &stream).unwrap();
                let documents = parse_rendered_yaml(&rendered.text).unwrap();
                assert_string_entry(&documents[0], "value", expected);
            }
        });
    }

    #[test]
    fn renders_block_chomping_and_indent_indicator_families() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "literal_strip=t'value: |-\\n  a\\n  b\\n'\nliteral_keep=t'value: |+\\n  a\\n  b\\n'\nliteral_keep_leading_blank=t'value: |+\\n\\n  a\\n'\nfolded_strip=t'value: >-\\n  a\\n  b\\n'\nfolded_keep=t'value: >+\\n  a\\n  b\\n'\nfolded_more=t'value: >\\n  a\\n    b\\n  c\\n'\nindent_indicator=t'value: |2\\n  a\\n  b\\n'\nliteral_blank_keep=t'value: |+\\n  a\\n\\n  b\\n'\nfolded_blank_keep=t'value: >+\\n  a\\n\\n  b\\n'\n"
                ),
                pyo3::ffi::c_str!("test_yaml_block_chomping_families.py"),
                pyo3::ffi::c_str!("test_yaml_block_chomping_families"),
            )
            .unwrap();

            for (name, expected_text, key, expected_value) in [
                ("literal_strip", "value: |-\n  a\n  b", "value", "a\nb"),
                ("literal_keep", "value: |+\n  a\n  b\n", "value", "a\nb\n"),
                (
                    "literal_keep_leading_blank",
                    "value: |+\n  \n  a\n",
                    "value",
                    "\na\n",
                ),
                ("folded_strip", "value: >-\n  a\n  b", "value", "a b"),
                ("folded_keep", "value: >+\n  a\n  b\n", "value", "a b\n"),
                (
                    "folded_more",
                    "value: >\n  a\n    b\n  c\n",
                    "value",
                    "a\n  b\nc\n",
                ),
                (
                    "indent_indicator",
                    "value: |2\n  a\n  b\n",
                    "value",
                    "a\nb\n",
                ),
                (
                    "literal_blank_keep",
                    "value: |+\n  a\n  \n  b\n",
                    "value",
                    "a\n\nb\n",
                ),
                (
                    "folded_blank_keep",
                    "value: >+\n  a\n  \n  b\n",
                    "value",
                    "a\nb\n",
                ),
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
                let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
                assert_eq!(rendered.text, expected_text, "{name}");
                let documents = parse_rendered_yaml(&rendered.text).unwrap();
                assert_string_entry(&documents[0], key, expected_value);
            }
        });
    }

    #[test]
    fn folds_multiline_plain_scalars() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("template=t'value: a\\n  b\\n\\n  c\\n'\n"),
                pyo3::ffi::c_str!("test_yaml_multiline_plain.py"),
                pyo3::ffi::c_str!("test_yaml_multiline_plain"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
            let stream = parse_template(&template).unwrap();
            let rendered = render_document(py, &stream).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_string_entry(&documents[0], "value", "a b\nc");
            assert!(rendered.text.contains("\"a b\\nc\""));
        });
    }

    #[test]
    fn accepts_top_level_flow_collections_with_trailing_newlines() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ntemplate=Template('{a: 1, b: [2, 3]}\\n')\n"
                ),
                pyo3::ffi::c_str!("test_yaml_flow_trailing_newline.py"),
                pyo3::ffi::c_str!("test_yaml_flow_trailing_newline"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
            let stream = parse_template(&template).unwrap();
            let rendered = render_document(py, &stream).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(
                documents[0]
                    .as_mapping()
                    .expect("expected YAML mapping")
                    .len(),
                2
            );
        });
    }

    #[test]
    fn accepts_line_wrapped_flow_collections() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nsequence=Template('key: [a,\\n  b]\\n')\nmapping=Template('key: {a: 1,\\n  b: 2}\\n')\n"
                ),
                pyo3::ffi::c_str!("test_yaml_wrapped_flow.py"),
                pyo3::ffi::c_str!("test_yaml_wrapped_flow"),
            )
            .unwrap();

            for name in ["sequence", "mapping"] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
                let stream = parse_template(&template).unwrap();
                let rendered = render_document(py, &stream).unwrap();
                let documents = parse_rendered_yaml(&rendered.text).unwrap();
                assert_eq!(
                    documents[0]
                        .as_mapping()
                        .expect("expected YAML mapping")
                        .len(),
                    1
                );
            }
        });
    }

    #[test]
    fn accepts_flow_collections_with_comments() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nsequence=Template('key: [a, # first\\n  b]\\n')\nmapping=Template('key: {a: 1, # first\\n  b: 2}\\n')\n"
                ),
                pyo3::ffi::c_str!("test_yaml_flow_comments.py"),
                pyo3::ffi::c_str!("test_yaml_flow_comments"),
            )
            .unwrap();

            for name in ["sequence", "mapping"] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
                let stream = parse_template(&template).unwrap();
                let rendered = render_document(py, &stream).unwrap();
                let documents = parse_rendered_yaml(&rendered.text).unwrap();
                assert_eq!(
                    documents[0]
                        .as_mapping()
                        .expect("expected YAML mapping")
                        .len(),
                    1
                );
            }
        });
    }

    #[test]
    fn treats_empty_documents_as_null() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nempty=Template('---\\n...\\n')\nstream=Template('---\\n\\n---\\na: 1\\n')\n"
                ),
                pyo3::ffi::c_str!("test_yaml_empty_docs.py"),
                pyo3::ffi::c_str!("test_yaml_empty_docs"),
            )
            .unwrap();

            let empty = module.getattr("empty").unwrap();
            let empty = extract_template(py, &empty, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&empty).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert!(documents.as_slice()[0].is_null());

            let stream = module.getattr("stream").unwrap();
            let stream = extract_template(py, &stream, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&stream).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert!(documents[0].is_null());
            assert!(documents[1].as_mapping().is_some());
        });
    }

    #[test]
    fn supports_indentless_sequence_values_and_empty_explicit_keys() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nindentless=Template('a:\\n- 1\\n- 2\\n')\nempty_key=Template('?\\n: 1\\n')\n"
                ),
                pyo3::ffi::c_str!("test_yaml_indentless_and_empty_key.py"),
                pyo3::ffi::c_str!("test_yaml_indentless_and_empty_key"),
            )
            .unwrap();

            let indentless = module.getattr("indentless").unwrap();
            let indentless = extract_template(py, &indentless, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&indentless).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let mapping = documents[0].as_mapping().expect("expected YAML mapping");
            let value = mapping
                .iter()
                .find_map(|(key, value)| (key.as_str() == Some("a")).then_some(value))
                .expect("expected key a");
            assert_eq!(value.as_vec().expect("expected YAML sequence").len(), 2);

            let empty_key = module.getattr("empty_key").unwrap();
            let empty_key = extract_template(py, &empty_key, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&empty_key).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let mapping = documents[0].as_mapping().expect("expected YAML mapping");
            let value = mapping
                .iter()
                .find_map(|(key, value)| key.is_null().then_some(value))
                .expect("expected null key");
            assert_eq!(value.as_integer(), Some(1));
        });
    }

    #[test]
    fn supports_compact_mappings_in_sequences_and_plain_hash_chars() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nseq=Template('- a: 1\\n  b: 2\\n- c: 3\\n')\nmapped=Template('items:\\n- a: 1\\n  b: 2\\n- c: 3\\n')\nseqs=Template('- - 1\\n  - 2\\n- - 3\\n')\nhash_value=Template('value: a#b\\n')\n"
                ),
                pyo3::ffi::c_str!("test_yaml_compact_sequence_maps.py"),
                pyo3::ffi::c_str!("test_yaml_compact_sequence_maps"),
            )
            .unwrap();

            for name in ["seq", "mapped", "seqs", "hash_value"] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
                let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
                let documents = parse_rendered_yaml(&rendered.text).unwrap();
                assert_eq!(documents.len(), 1);
            }
        });
    }

    #[test]
    fn preserves_explicit_document_end_markers_in_streams() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ntemplate=Template('---\\na: 1\\n...\\n---\\nb: 2\\n')\ncommented=Template('---\\na: 1\\n... # end\\n---\\nb: 2\\n')\n"
                ),
                pyo3::ffi::c_str!("test_yaml_explicit_end.py"),
                pyo3::ffi::c_str!("test_yaml_explicit_end"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
            let stream = parse_template(&template).unwrap();
            let rendered = render_document(py, &stream).unwrap();

            assert!(rendered.text.contains("...\n---"));

            let commented = module.getattr("commented").unwrap();
            let commented = extract_template(py, &commented, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&commented).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(documents.len(), 2);
        });
    }

    #[test]
    fn parses_verbatim_tags() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ntemplate=Template('value: !<tag:yaml.org,2002:str> hello\\n')\n"
                ),
                pyo3::ffi::c_str!("test_yaml_verbatim_tag.py"),
                pyo3::ffi::c_str!("test_yaml_verbatim_tag"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
            let stream = parse_template(&template).unwrap();
            let rendered = render_document(py, &stream).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();

            assert!(rendered.text.contains("!<tag:yaml.org,2002:str>"));
            assert_string_entry(&documents[0], "value", "hello");
        });
    }

    #[test]
    fn validates_user_defined_tags_via_saphyr() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nscalar=Template('!custom 3\\n')\ncustom_tag_scalar=Template('value: !custom 3\\n')\ncustom_tag_sequence=Template('value: !custom [1, 2]\\n')\ncommented_root=Template('--- # comment\\n!custom [1, 2]\\n')\n"
                ),
                pyo3::ffi::c_str!("test_yaml_custom_tags.py"),
                pyo3::ffi::c_str!("test_yaml_custom_tags"),
            )
            .unwrap();

            let scalar = module.getattr("scalar").unwrap();
            let scalar = extract_template(py, &scalar, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&scalar).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(rendered.text.trim_end(), "!custom 3");
            assert!(rendered.text.contains("!custom 3"));
            assert_eq!(yaml_integer(&documents[0]), Some(3));

            let custom_tag_scalar = module.getattr("custom_tag_scalar").unwrap();
            let custom_tag_scalar =
                extract_template(py, &custom_tag_scalar, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&custom_tag_scalar).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(rendered.text.trim_end(), "value: !custom 3");
            assert!(rendered.text.contains("value: !custom 3"));
            assert_integer_entry(&documents[0], "value", 3);

            let custom_tag_sequence = module.getattr("custom_tag_sequence").unwrap();
            let custom_tag_sequence =
                extract_template(py, &custom_tag_sequence, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&custom_tag_sequence).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert!(rendered.text.contains("value: !custom [ 1, 2 ]"));
            let value = documents[0]
                .as_mapping()
                .expect("expected YAML mapping")
                .iter()
                .find_map(|(key, value)| (yaml_scalar_text(key) == Some("value")).then_some(value))
                .expect("expected value key");
            assert_eq!(yaml_sequence_len(value), Some(2));

            let commented_root = module.getattr("commented_root").unwrap();
            let commented_root =
                extract_template(py, &commented_root, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&commented_root).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(yaml_sequence_len(&documents[0]), Some(2));
        });
    }

    #[test]
    fn rejects_aliases_that_cross_document_boundaries() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nstream=Template('--- &a\\n- 1\\n- 2\\n---\\n*a\\n')\n"
                ),
                pyo3::ffi::c_str!("test_yaml_cross_doc_alias.py"),
                pyo3::ffi::c_str!("test_yaml_cross_doc_alias"),
            )
            .unwrap();

            let stream = module.getattr("stream").unwrap();
            let stream = extract_template(py, &stream, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&stream).unwrap()).unwrap();
            let err = parse_rendered_yaml(&rendered.text).expect_err("expected YAML parse failure");
            assert_eq!(err.kind, ErrorKind::Parse);
            assert!(err.message.contains("unknown anchor"));
        });
    }

    #[test]
    fn preserves_tag_directives_and_handle_tags() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nscalar=Template('%TAG !e! tag:example.com,2020:\\n---\\nvalue: !e!foo 1\\n')\ndoc_start_comment=Template('--- # comment\\nvalue: 1\\n')\ndoc_start_tag_comment=Template('--- !!str true # comment\\n')\nroot=Template('%YAML 1.2\\n%TAG !e! tag:example.com,2020:\\n---\\n!e!root {value: !e!leaf 1}\\n')\nblock_map=Template('--- !!map\\na: 1\\n')\nblock_seq=Template('--- !!seq\\n- 1\\n- 2\\n')\nanchor_map=Template('--- &root\\n  a: 1\\n')\nanchor_seq=Template('--- !custom &root\\n  - 1\\n  - 2\\n')\n"
                ),
                pyo3::ffi::c_str!("test_yaml_tag_directives.py"),
                pyo3::ffi::c_str!("test_yaml_tag_directives"),
            )
            .unwrap();

            let scalar = module.getattr("scalar").unwrap();
            let scalar = extract_template(py, &scalar, "yaml_t/yaml_t_str").unwrap();
            let stream = parse_template(&scalar).unwrap();
            let rendered = render_document(py, &stream).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(
                rendered.text,
                "%TAG !e! tag:example.com,2020:\n---\nvalue: !e!foo 1"
            );
            assert_integer_entry(&documents[0], "value", 1);

            let doc_start_comment = module.getattr("doc_start_comment").unwrap();
            let doc_start_comment =
                extract_template(py, &doc_start_comment, "yaml_t/yaml_t_str").unwrap();
            let stream = parse_template(&doc_start_comment).unwrap();
            let rendered = render_document(py, &stream).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(rendered.text, "---\nvalue: 1");
            assert_integer_entry(&documents[0], "value", 1);

            let doc_start_tag_comment = module.getattr("doc_start_tag_comment").unwrap();
            let doc_start_tag_comment =
                extract_template(py, &doc_start_tag_comment, "yaml_t/yaml_t_str").unwrap();
            let stream = parse_template(&doc_start_tag_comment).unwrap();
            let rendered = render_document(py, &stream).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(rendered.text, "---\n!!str true");
            assert_eq!(yaml_scalar_text(&documents[0]), Some("true"));

            let root = module.getattr("root").unwrap();
            let root = extract_template(py, &root, "yaml_t/yaml_t_str").unwrap();
            let stream = parse_template(&root).unwrap();
            let rendered = render_document(py, &stream).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(
                rendered.text,
                "%YAML 1.2\n%TAG !e! tag:example.com,2020:\n---\n!e!root { value: !e!leaf 1 }"
            );
            assert_integer_entry(&documents[0], "value", 1);

            let block_map = module.getattr("block_map").unwrap();
            let block_map = extract_template(py, &block_map, "yaml_t/yaml_t_str").unwrap();
            let stream = parse_template(&block_map).unwrap();
            let rendered = render_document(py, &stream).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(rendered.text, "---\n!!map\na: 1");
            assert_integer_entry(&documents[0], "a", 1);

            let block_seq = module.getattr("block_seq").unwrap();
            let block_seq = extract_template(py, &block_seq, "yaml_t/yaml_t_str").unwrap();
            let stream = parse_template(&block_seq).unwrap();
            let rendered = render_document(py, &stream).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(rendered.text, "---\n!!seq\n- 1\n- 2");
            assert_eq!(yaml_sequence_len(&documents[0]), Some(2));

            let anchor_map = module.getattr("anchor_map").unwrap();
            let anchor_map = extract_template(py, &anchor_map, "yaml_t/yaml_t_str").unwrap();
            let stream = parse_template(&anchor_map).unwrap();
            let rendered = render_document(py, &stream).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(rendered.text, "---\n&root\na: 1");
            assert_integer_entry(&documents[0], "a", 1);

            let anchor_seq = module.getattr("anchor_seq").unwrap();
            let anchor_seq = extract_template(py, &anchor_seq, "yaml_t/yaml_t_str").unwrap();
            let stream = parse_template(&anchor_seq).unwrap();
            let rendered = render_document(py, &stream).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(rendered.text, "---\n!custom &root\n- 1\n- 2");
            assert_eq!(yaml_sequence_len(&documents[0]), Some(2));
        });
    }

    #[test]
    fn preserves_explicit_core_schema_tags() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nmapping=Template('value_bool: !!bool true\\nvalue_str: !!str true\\nvalue_float: !!float 1\\nvalue_null: !!null null\\n')\nroot_int=Template('--- !!int 3\\n')\nroot_str=Template('--- !!str true\\n')\nroot_bool=Template('--- !!bool true\\n')\n"
                ),
                pyo3::ffi::c_str!("test_yaml_core_tags.py"),
                pyo3::ffi::c_str!("test_yaml_core_tags"),
            )
            .unwrap();

            let mapping = module.getattr("mapping").unwrap();
            let mapping = extract_template(py, &mapping, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&mapping).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let mapping = yaml_mapping(&documents[0]).expect("expected YAML mapping");
            let float_value = mapping
                .iter()
                .find_map(|(key, value)| {
                    (yaml_scalar_text(key) == Some("value_float")).then_some(value)
                })
                .expect("expected value_float key");
            assert_eq!(yaml_float(float_value), Some(1.0));
            assert_string_entry(&documents[0], "value_str", "true");

            let root_int = module.getattr("root_int").unwrap();
            let root_int = extract_template(py, &root_int, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&root_int).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(yaml_integer(&documents[0]), Some(3));

            let root_str = module.getattr("root_str").unwrap();
            let root_str = extract_template(py, &root_str, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&root_str).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(yaml_scalar_text(&documents[0]), Some("true"));

            let root_bool = module.getattr("root_bool").unwrap();
            let root_bool = extract_template(py, &root_bool, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&root_bool).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert!(documents[0].is_boolean());
        });
    }

    #[test]
    fn supports_flow_trailing_commas_sequence_values_and_indent_indicators() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ncomment_only=Template('# comment\\n')\ncomment_only_explicit=Template('--- # comment\\n')\ncomment_only_explicit_end=Template('--- # comment\\n...\\n')\ncomment_only_explicit_end_stream=Template('--- # comment\\n...\\n---\\na: 1\\n')\ncomment_only_mid_stream=Template('---\\na: 1\\n--- # comment\\n...\\n---\\nb: 2\\n')\ncomment_only_tail_stream=Template('---\\na: 1\\n--- # comment\\n...\\n')\nflow_seq=Template('[1, 2,]\\n')\nempty_flow_seq=Template('value: []\\n')\nflow_map=Template('{a: 1,}\\n')\nempty_flow_map=Template('value: {}\\n')\nflow_scalar_mix=Template('value: [\"\", \\'\\', plain]\\n')\nflow_plain_scalar=Template('value: [1 2]\\n')\nflow_hash_plain_mapping_value=Template('value: {a: b#c}\\n')\nflow_hash_plain_mapping_values=Template('value: {a: b#c, d: e#f}\\n')\nflow_hash_plain_scalars=Template('value: [a#b, c#d]\\n')\nflow_hash_value_sequence=Template('value: [a#b, c#d, e#f]\\n')\nflow_hash_long_sequence=Template('value: [a#b, c#d, e#f, g#h]\\n')\nflow_hash_five_sequence=Template('value: [a#b, c#d, e#f, g#h, i#j]\\n')\nflow_hash_seq_six=Template('value: [a#b, c#d, e#f, g#h, i#j, k#l]\\n')\nflow_hash_seq_seven=Template('value: [a#b, c#d, e#f, g#h, i#j, k#l, m#n]\\n')\nflow_mapping_hash_key=Template('value: {a#b: 1}\\n')\nflow_sequence_comments_value=Template('value: [1, # c\\n 2]\\n')\nflow_mapping_comments_value=Template('value: {a: 1, # c\\n b: 2}\\n')\ncomment_after_value=Template('value: a # c\\n')\nplain_colon_hash=Template('value: a:b#c\\n')\nplain_colon_hash_deeper=Template('value: a:b:c#d\\n')\nplain_hash_chain=Template('value: a#b#c\\n')\nplain_hash_chain_deeper=Template('value: a#b#c#d\\n')\nplain_hash_chain_deeper_comment=Template('value: a#b#c#d # comment\\n')\nflow_hash_mapping_long=Template('value: {a: b#c, d: e#f, g: h#i}\\n')\nflow_hash_mapping_four=Template('value: {a: b#c, d: e#f, g: h#i, j: k#l}\\n')\nflow_hash_mapping_five=Template('value: {a: b#c, d: e#f, g: h#i, j: k#l, m: n#o}\\n')\nflow_hash_mapping_six=Template('value: {a: b#c, d: e#f, g: h#i, j: k#l, m: n#o, p: q#r}\\n')\ncomment_after_plain_colon=Template('value: a:b # c\\n')\ncomment_after_flow_plain_colon=Template('value: [a:b # c\\n]\\n')\nflow_plain_hash_chain=Template('value: [a#b#c, d#e#f]\\n')\nflow_plain_hash_chain_single_deeper=Template('value: [a#b#c#d]\\n')\nflow_plain_hash_chain_single_deeper_comment=Template('value: [a#b#c#d # comment\\n]\\n')\nflow_plain_hash_chain_long=Template('value: [a#b#c, d#e#f, g#h#i]\\n')\nflow_plain_hash_chain_four=Template('value: [a#b#c, d#e#f, g#h#i, j#k#l]\\n')\nblock_plain_comment_after_colon_long=Template('value: a:b:c # comment\\n')\nblock_plain_comment_after_colon_deeper=Template('value: a:b:c:d # comment\\n')\nflow_plain_comment_after_colon_long=Template('value: [a:b:c # comment\\n]\\n')\nflow_plain_comment_after_colon_deeper=Template('value: [a:b:c:d # comment\\n]\\n')\nflow_plain_colon_hash_deeper=Template('value: [a:b:c#d]\\n')\nflow_mapping_plain_key_question=Template('value: {?x: 1}\\n')\nflow_mapping_plain_key_questions=Template('value: {?x: 1, ?y: 2}\\n')\nmapping_empty_flow_values=Template('value: {a: [], b: {}}\\n')\nflow_mapping_empty_key=Template('{\"\": 1}\\n')\nflow_mapping_empty_key_and_values=Template('{\"\": [], foo: {}}\\n')\nflow_mapping_nested_empty=Template('{a: {}, b: []}\\n')\nflow_null_key=Template('{null: 1, \"\": 2}\\n')\nflow_sequence_nested_empty=Template('[[], {}]\\n')\nplain_scalar_colon_no_space=Template('value: a:b\\n')\nplain_question_mark_scalar=Template('value: ?x\\n')\nplain_colon_scalar_flow=Template('value: [a:b, c:d]\\n')\nflow_mapping_colon_plain_key=Template('value: {a:b: c}\\n')\nflow_mapping_colon_and_hash=Template('value: {a:b: c#d}\\n')\nblock_plain_colon_no_space=Template('value: a:b:c\\n')\nblock_null_key=Template('? null\\n: 1\\n')\nquoted_null_key=Template('? \"\"\\n: 1\\n')\nalias_in_flow_mapping_value=Template('base: &a {x: 1}\\nvalue: {ref: *a}\\n')\nflow_null_and_alias=Template('base: &a {x: 1}\\nvalue: {null: *a}\\n')\nalias_seq_value=Template('a: &x [1, 2]\\nb: *x\\n')\nflow_mapping_missing_value=Template('value: {a: }\\n')\nflow_seq_missing_value_before_end=Template('value: [1, 2, ]\\n')\nflow_alias_map=Template('value: {left: &a 1, right: *a}\\n')\nflow_alias_seq=Template('value: [&a 1, *a]\\n')\nflow_merge=Template('value: {<<: &base {a: 1}, b: 2}\\n')\nnested_flow_alias_merge=Template('value: [{<<: &base {a: 1}, b: 2}, *base]\\n')\nexplicit_seq=Template('? a\\n: - 1\\n  - 2\\n')\nindented_block=Template('value: |1\\n a\\n b\\n')\n"
                ),
                pyo3::ffi::c_str!("test_yaml_edge_cases.py"),
                pyo3::ffi::c_str!("test_yaml_edge_cases"),
            )
            .unwrap();

            let flow_seq = module.getattr("flow_seq").unwrap();
            let flow_seq = extract_template(py, &flow_seq, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&flow_seq).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(yaml_sequence_len(&documents[0]), Some(2));

            let comment_only = module.getattr("comment_only").unwrap();
            let comment_only = extract_template(py, &comment_only, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&comment_only).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert!(documents[0].is_null());

            let comment_only_explicit = module.getattr("comment_only_explicit").unwrap();
            let comment_only_explicit =
                extract_template(py, &comment_only_explicit, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&comment_only_explicit).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert!(documents[0].is_null());

            let comment_only_explicit_end = module.getattr("comment_only_explicit_end").unwrap();
            let comment_only_explicit_end =
                extract_template(py, &comment_only_explicit_end, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&comment_only_explicit_end).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert!(documents[0].is_null());

            let comment_only_explicit_end_stream =
                module.getattr("comment_only_explicit_end_stream").unwrap();
            let comment_only_explicit_end_stream =
                extract_template(py, &comment_only_explicit_end_stream, "yaml_t/yaml_t_str")
                    .unwrap();
            let rendered = render_document(
                py,
                &parse_template(&comment_only_explicit_end_stream).unwrap(),
            )
            .unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(documents.len(), 2);
            assert!(documents[0].is_null());
            assert_integer_entry(&documents[1], "a", 1);

            let comment_only_mid_stream = module.getattr("comment_only_mid_stream").unwrap();
            let comment_only_mid_stream =
                extract_template(py, &comment_only_mid_stream, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&comment_only_mid_stream).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(documents.len(), 3);
            assert_integer_entry(&documents[0], "a", 1);
            assert!(documents[1].is_null());
            assert_integer_entry(&documents[2], "b", 2);

            let comment_only_tail_stream = module.getattr("comment_only_tail_stream").unwrap();
            let comment_only_tail_stream =
                extract_template(py, &comment_only_tail_stream, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&comment_only_tail_stream).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(documents.len(), 2);
            assert_integer_entry(&documents[0], "a", 1);
            assert!(documents[1].is_null());

            let flow_map = module.getattr("flow_map").unwrap();
            let flow_map = extract_template(py, &flow_map, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&flow_map).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_integer_entry(&documents[0], "a", 1);

            let empty_flow_seq = module.getattr("empty_flow_seq").unwrap();
            let empty_flow_seq =
                extract_template(py, &empty_flow_seq, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&empty_flow_seq).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            assert_eq!(yaml_sequence_len(value), Some(0));

            let empty_flow_map = module.getattr("empty_flow_map").unwrap();
            let empty_flow_map =
                extract_template(py, &empty_flow_map, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&empty_flow_map).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            assert!(yaml_mapping(value).is_some());

            let flow_scalar_mix = module.getattr("flow_scalar_mix").unwrap();
            let flow_scalar_mix =
                extract_template(py, &flow_scalar_mix, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&flow_scalar_mix).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            match value {
                YamlOwned::Sequence(sequence) => {
                    assert_eq!(sequence.len(), 3);
                    assert_eq!(yaml_scalar_text(&sequence[0]), Some(""));
                    assert_eq!(yaml_scalar_text(&sequence[1]), Some(""));
                    assert_eq!(yaml_scalar_text(&sequence[2]), Some("plain"));
                }
                _ => panic!("expected YAML sequence"),
            }

            let flow_plain_scalar = module.getattr("flow_plain_scalar").unwrap();
            let flow_plain_scalar =
                extract_template(py, &flow_plain_scalar, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_plain_scalar).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            match value {
                YamlOwned::Sequence(sequence) => {
                    assert_eq!(sequence.len(), 1);
                    assert_eq!(yaml_scalar_text(&sequence[0]), Some("1 2"));
                }
                _ => panic!("expected YAML sequence"),
            }

            let flow_hash_plain_mapping_value =
                module.getattr("flow_hash_plain_mapping_value").unwrap();
            let flow_hash_plain_mapping_value =
                extract_template(py, &flow_hash_plain_mapping_value, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_hash_plain_mapping_value).unwrap())
                    .unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            assert_string_entry(value, "a", "b#c");

            let flow_hash_plain_mapping_values =
                module.getattr("flow_hash_plain_mapping_values").unwrap();
            let flow_hash_plain_mapping_values =
                extract_template(py, &flow_hash_plain_mapping_values, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(
                py,
                &parse_template(&flow_hash_plain_mapping_values).unwrap(),
            )
            .unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            assert_string_entry(value, "a", "b#c");
            assert_string_entry(value, "d", "e#f");

            let flow_hash_plain_scalars = module.getattr("flow_hash_plain_scalars").unwrap();
            let flow_hash_plain_scalars =
                extract_template(py, &flow_hash_plain_scalars, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_hash_plain_scalars).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            match value {
                YamlOwned::Sequence(sequence) => {
                    assert_eq!(sequence.len(), 2);
                    assert_eq!(yaml_scalar_text(&sequence[0]), Some("a#b"));
                    assert_eq!(yaml_scalar_text(&sequence[1]), Some("c#d"));
                }
                _ => panic!("expected YAML sequence"),
            }

            let flow_hash_value_sequence = module.getattr("flow_hash_value_sequence").unwrap();
            let flow_hash_value_sequence =
                extract_template(py, &flow_hash_value_sequence, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_hash_value_sequence).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            match value {
                YamlOwned::Sequence(sequence) => {
                    assert_eq!(sequence.len(), 3);
                    assert_eq!(yaml_scalar_text(&sequence[0]), Some("a#b"));
                    assert_eq!(yaml_scalar_text(&sequence[1]), Some("c#d"));
                    assert_eq!(yaml_scalar_text(&sequence[2]), Some("e#f"));
                }
                _ => panic!("expected YAML sequence"),
            }

            let flow_hash_long_sequence = module.getattr("flow_hash_long_sequence").unwrap();
            let flow_hash_long_sequence =
                extract_template(py, &flow_hash_long_sequence, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_hash_long_sequence).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            match value {
                YamlOwned::Sequence(sequence) => {
                    assert_eq!(sequence.len(), 4);
                    assert_eq!(yaml_scalar_text(&sequence[3]), Some("g#h"));
                }
                _ => panic!("expected YAML sequence"),
            }

            let flow_hash_five_sequence = module.getattr("flow_hash_five_sequence").unwrap();
            let flow_hash_five_sequence =
                extract_template(py, &flow_hash_five_sequence, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_hash_five_sequence).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            match value {
                YamlOwned::Sequence(sequence) => {
                    assert_eq!(sequence.len(), 5);
                    assert_eq!(yaml_scalar_text(&sequence[4]), Some("i#j"));
                }
                _ => panic!("expected YAML sequence"),
            }

            let flow_mapping_hash_key = module.getattr("flow_mapping_hash_key").unwrap();
            let flow_mapping_hash_key =
                extract_template(py, &flow_mapping_hash_key, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_mapping_hash_key).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            assert_integer_entry(value, "a#b", 1);

            let flow_sequence_comments_value =
                module.getattr("flow_sequence_comments_value").unwrap();
            let flow_sequence_comments_value =
                extract_template(py, &flow_sequence_comments_value, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_sequence_comments_value).unwrap())
                    .unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            match value {
                YamlOwned::Sequence(sequence) => {
                    assert_eq!(sequence.len(), 2);
                    assert_eq!(yaml_integer(&sequence[0]), Some(1));
                    assert_eq!(yaml_integer(&sequence[1]), Some(2));
                }
                _ => panic!("expected YAML sequence"),
            }

            let flow_mapping_comments_value =
                module.getattr("flow_mapping_comments_value").unwrap();
            let flow_mapping_comments_value =
                extract_template(py, &flow_mapping_comments_value, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_mapping_comments_value).unwrap())
                    .unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            assert_integer_entry(value, "a", 1);
            assert_integer_entry(value, "b", 2);

            let comment_after_value = module.getattr("comment_after_value").unwrap();
            let comment_after_value =
                extract_template(py, &comment_after_value, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&comment_after_value).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_string_entry(&documents[0], "value", "a");

            let plain_colon_hash = module.getattr("plain_colon_hash").unwrap();
            let plain_colon_hash =
                extract_template(py, &plain_colon_hash, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&plain_colon_hash).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_string_entry(&documents[0], "value", "a:b#c");

            let plain_colon_hash_deeper = module.getattr("plain_colon_hash_deeper").unwrap();
            let plain_colon_hash_deeper =
                extract_template(py, &plain_colon_hash_deeper, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&plain_colon_hash_deeper).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_string_entry(&documents[0], "value", "a:b:c#d");

            let plain_hash_chain = module.getattr("plain_hash_chain").unwrap();
            let plain_hash_chain =
                extract_template(py, &plain_hash_chain, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&plain_hash_chain).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_string_entry(&documents[0], "value", "a#b#c");

            let plain_hash_chain_deeper = module.getattr("plain_hash_chain_deeper").unwrap();
            let plain_hash_chain_deeper =
                extract_template(py, &plain_hash_chain_deeper, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&plain_hash_chain_deeper).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_string_entry(&documents[0], "value", "a#b#c#d");

            let plain_hash_chain_deeper_comment =
                module.getattr("plain_hash_chain_deeper_comment").unwrap();
            let plain_hash_chain_deeper_comment =
                extract_template(py, &plain_hash_chain_deeper_comment, "yaml_t/yaml_t_str")
                    .unwrap();
            let rendered = render_document(
                py,
                &parse_template(&plain_hash_chain_deeper_comment).unwrap(),
            )
            .unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_string_entry(&documents[0], "value", "a#b#c#d");

            let flow_hash_mapping_long = module.getattr("flow_hash_mapping_long").unwrap();
            let flow_hash_mapping_long =
                extract_template(py, &flow_hash_mapping_long, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_hash_mapping_long).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            assert_string_entry(value, "a", "b#c");
            assert_string_entry(value, "d", "e#f");
            assert_string_entry(value, "g", "h#i");

            let flow_hash_mapping_four = module.getattr("flow_hash_mapping_four").unwrap();
            let flow_hash_mapping_four =
                extract_template(py, &flow_hash_mapping_four, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_hash_mapping_four).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            assert_string_entry(value, "a", "b#c");
            assert_string_entry(value, "d", "e#f");
            assert_string_entry(value, "g", "h#i");
            assert_string_entry(value, "j", "k#l");

            let flow_hash_mapping_five = module.getattr("flow_hash_mapping_five").unwrap();
            let flow_hash_mapping_five =
                extract_template(py, &flow_hash_mapping_five, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_hash_mapping_five).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            assert_string_entry(value, "m", "n#o");

            let flow_hash_mapping_six = module.getattr("flow_hash_mapping_six").unwrap();
            let flow_hash_mapping_six =
                extract_template(py, &flow_hash_mapping_six, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_hash_mapping_six).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            assert_string_entry(value, "p", "q#r");

            let comment_after_plain_colon = module.getattr("comment_after_plain_colon").unwrap();
            let comment_after_plain_colon =
                extract_template(py, &comment_after_plain_colon, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&comment_after_plain_colon).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_string_entry(&documents[0], "value", "a:b");

            let comment_after_flow_plain_colon =
                module.getattr("comment_after_flow_plain_colon").unwrap();
            let comment_after_flow_plain_colon =
                extract_template(py, &comment_after_flow_plain_colon, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(
                py,
                &parse_template(&comment_after_flow_plain_colon).unwrap(),
            )
            .unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            match value {
                YamlOwned::Sequence(sequence) => {
                    assert_eq!(sequence.len(), 1);
                    assert_eq!(yaml_scalar_text(&sequence[0]), Some("a:b"));
                }
                _ => panic!("expected YAML sequence"),
            }

            let flow_plain_hash_chain = module.getattr("flow_plain_hash_chain").unwrap();
            let flow_plain_hash_chain =
                extract_template(py, &flow_plain_hash_chain, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_plain_hash_chain).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            match value {
                YamlOwned::Sequence(sequence) => {
                    assert_eq!(sequence.len(), 2);
                    assert_eq!(yaml_scalar_text(&sequence[0]), Some("a#b#c"));
                    assert_eq!(yaml_scalar_text(&sequence[1]), Some("d#e#f"));
                }
                _ => panic!("expected YAML sequence"),
            }

            let flow_plain_hash_chain_single_deeper = module
                .getattr("flow_plain_hash_chain_single_deeper")
                .unwrap();
            let flow_plain_hash_chain_single_deeper = extract_template(
                py,
                &flow_plain_hash_chain_single_deeper,
                "yaml_t/yaml_t_str",
            )
            .unwrap();
            let rendered = render_document(
                py,
                &parse_template(&flow_plain_hash_chain_single_deeper).unwrap(),
            )
            .unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            match value {
                YamlOwned::Sequence(sequence) => {
                    assert_eq!(sequence.len(), 1);
                    assert_eq!(yaml_scalar_text(&sequence[0]), Some("a#b#c#d"));
                }
                _ => panic!("expected YAML sequence"),
            }

            let flow_plain_hash_chain_single_deeper_comment = module
                .getattr("flow_plain_hash_chain_single_deeper_comment")
                .unwrap();
            let flow_plain_hash_chain_single_deeper_comment = extract_template(
                py,
                &flow_plain_hash_chain_single_deeper_comment,
                "yaml_t/yaml_t_str",
            )
            .unwrap();
            let rendered = render_document(
                py,
                &parse_template(&flow_plain_hash_chain_single_deeper_comment).unwrap(),
            )
            .unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            match value {
                YamlOwned::Sequence(sequence) => {
                    assert_eq!(sequence.len(), 1);
                    assert_eq!(yaml_scalar_text(&sequence[0]), Some("a#b#c#d"));
                }
                _ => panic!("expected YAML sequence"),
            }

            let flow_hash_seq_six = module.getattr("flow_hash_seq_six").unwrap();
            let flow_hash_seq_six =
                extract_template(py, &flow_hash_seq_six, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_hash_seq_six).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            match value {
                YamlOwned::Sequence(sequence) => {
                    assert_eq!(sequence.len(), 6);
                    assert_eq!(yaml_scalar_text(&sequence[5]), Some("k#l"));
                }
                _ => panic!("expected YAML sequence"),
            }

            let flow_hash_seq_seven = module.getattr("flow_hash_seq_seven").unwrap();
            let flow_hash_seq_seven =
                extract_template(py, &flow_hash_seq_seven, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_hash_seq_seven).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            match value {
                YamlOwned::Sequence(sequence) => {
                    assert_eq!(sequence.len(), 7);
                    assert_eq!(yaml_scalar_text(&sequence[6]), Some("m#n"));
                }
                _ => panic!("expected YAML sequence"),
            }

            let flow_plain_hash_chain_four = module.getattr("flow_plain_hash_chain_four").unwrap();
            let flow_plain_hash_chain_four =
                extract_template(py, &flow_plain_hash_chain_four, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_plain_hash_chain_four).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            match value {
                YamlOwned::Sequence(sequence) => {
                    assert_eq!(sequence.len(), 4);
                    assert_eq!(yaml_scalar_text(&sequence[3]), Some("j#k#l"));
                }
                _ => panic!("expected YAML sequence"),
            }

            let block_plain_comment_after_colon_deeper = module
                .getattr("block_plain_comment_after_colon_deeper")
                .unwrap();
            let block_plain_comment_after_colon_deeper = extract_template(
                py,
                &block_plain_comment_after_colon_deeper,
                "yaml_t/yaml_t_str",
            )
            .unwrap();
            let rendered = render_document(
                py,
                &parse_template(&block_plain_comment_after_colon_deeper).unwrap(),
            )
            .unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_string_entry(&documents[0], "value", "a:b:c:d");

            let flow_plain_comment_after_colon_deeper = module
                .getattr("flow_plain_comment_after_colon_deeper")
                .unwrap();
            let flow_plain_comment_after_colon_deeper = extract_template(
                py,
                &flow_plain_comment_after_colon_deeper,
                "yaml_t/yaml_t_str",
            )
            .unwrap();
            let rendered = render_document(
                py,
                &parse_template(&flow_plain_comment_after_colon_deeper).unwrap(),
            )
            .unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            match value {
                YamlOwned::Sequence(sequence) => {
                    assert_eq!(sequence.len(), 1);
                    assert_eq!(yaml_scalar_text(&sequence[0]), Some("a:b:c:d"));
                }
                _ => panic!("expected YAML sequence"),
            }

            let flow_mapping_plain_key_question =
                module.getattr("flow_mapping_plain_key_question").unwrap();
            let flow_mapping_plain_key_question =
                extract_template(py, &flow_mapping_plain_key_question, "yaml_t/yaml_t_str")
                    .unwrap();
            let rendered = render_document(
                py,
                &parse_template(&flow_mapping_plain_key_question).unwrap(),
            )
            .unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            let mapping = yaml_mapping(value).expect("expected YAML mapping");
            assert_eq!(mapping.len(), 1);
            let (_, entry_value) = mapping.iter().next().expect("expected YAML mapping entry");
            assert_eq!(yaml_integer(entry_value), Some(1));

            let flow_mapping_plain_key_questions =
                module.getattr("flow_mapping_plain_key_questions").unwrap();
            let flow_mapping_plain_key_questions =
                extract_template(py, &flow_mapping_plain_key_questions, "yaml_t/yaml_t_str")
                    .unwrap();
            let rendered = render_document(
                py,
                &parse_template(&flow_mapping_plain_key_questions).unwrap(),
            )
            .unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            let mapping = yaml_mapping(value).expect("expected YAML mapping");
            assert_eq!(mapping.len(), 2);

            let mapping_empty_flow_values = module.getattr("mapping_empty_flow_values").unwrap();
            let mapping_empty_flow_values =
                extract_template(py, &mapping_empty_flow_values, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&mapping_empty_flow_values).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            let mapping = yaml_mapping(value).expect("expected YAML mapping");
            let left = mapping
                .iter()
                .find_map(|(key, value)| (yaml_scalar_text(key) == Some("a")).then_some(value))
                .expect("expected key a");
            let right = mapping
                .iter()
                .find_map(|(key, value)| (yaml_scalar_text(key) == Some("b")).then_some(value))
                .expect("expected key b");
            assert_eq!(yaml_sequence_len(left), Some(0));
            assert!(yaml_mapping(right).is_some());

            let flow_mapping_empty_key = module.getattr("flow_mapping_empty_key").unwrap();
            let flow_mapping_empty_key =
                extract_template(py, &flow_mapping_empty_key, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_mapping_empty_key).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_integer_entry(&documents[0], "", 1);

            let flow_mapping_empty_key_and_values =
                module.getattr("flow_mapping_empty_key_and_values").unwrap();
            let flow_mapping_empty_key_and_values =
                extract_template(py, &flow_mapping_empty_key_and_values, "yaml_t/yaml_t_str")
                    .unwrap();
            let rendered = render_document(
                py,
                &parse_template(&flow_mapping_empty_key_and_values).unwrap(),
            )
            .unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert!(yaml_mapping_entry(&documents[0], "").is_some());
            let foo = yaml_mapping_entry(&documents[0], "foo").expect("expected foo");
            assert!(yaml_mapping(foo).is_some());

            let flow_mapping_nested_empty = module.getattr("flow_mapping_nested_empty").unwrap();
            let flow_mapping_nested_empty =
                extract_template(py, &flow_mapping_nested_empty, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_mapping_nested_empty).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let a = yaml_mapping_entry(&documents[0], "a").expect("expected key a");
            let b = yaml_mapping_entry(&documents[0], "b").expect("expected key b");
            assert!(yaml_mapping(a).is_some());
            assert_eq!(yaml_sequence_len(b), Some(0));

            let flow_null_key = module.getattr("flow_null_key").unwrap();
            let flow_null_key = extract_template(py, &flow_null_key, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&flow_null_key).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let mapping = yaml_mapping(&documents[0]).expect("expected YAML mapping");
            assert!(
                mapping
                    .iter()
                    .any(|(key, value)| key.is_null() && yaml_integer(value) == Some(1))
            );
            assert!(mapping.iter().any(|(key, value)| {
                yaml_scalar_text(key) == Some("") && yaml_integer(value) == Some(2)
            }));

            let block_null_key = module.getattr("block_null_key").unwrap();
            let block_null_key =
                extract_template(py, &block_null_key, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&block_null_key).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let mapping = yaml_mapping(&documents[0]).expect("expected YAML mapping");
            assert!(
                mapping
                    .iter()
                    .any(|(key, value)| key.is_null() && yaml_integer(value) == Some(1))
            );

            let quoted_null_key = module.getattr("quoted_null_key").unwrap();
            let quoted_null_key =
                extract_template(py, &quoted_null_key, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&quoted_null_key).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_integer_entry(&documents[0], "", 1);

            let flow_sequence_nested_empty = module.getattr("flow_sequence_nested_empty").unwrap();
            let flow_sequence_nested_empty =
                extract_template(py, &flow_sequence_nested_empty, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_sequence_nested_empty).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(yaml_sequence_len(&documents[0]), Some(2));

            let plain_scalar_colon_no_space =
                module.getattr("plain_scalar_colon_no_space").unwrap();
            let plain_scalar_colon_no_space =
                extract_template(py, &plain_scalar_colon_no_space, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&plain_scalar_colon_no_space).unwrap())
                    .unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_string_entry(&documents[0], "value", "a:b");

            let plain_question_mark_scalar = module.getattr("plain_question_mark_scalar").unwrap();
            let plain_question_mark_scalar =
                extract_template(py, &plain_question_mark_scalar, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&plain_question_mark_scalar).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_string_entry(&documents[0], "value", "?x");

            let plain_colon_scalar_flow = module.getattr("plain_colon_scalar_flow").unwrap();
            let plain_colon_scalar_flow =
                extract_template(py, &plain_colon_scalar_flow, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&plain_colon_scalar_flow).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            assert_eq!(yaml_sequence_len(value), Some(2));

            let flow_mapping_colon_plain_key =
                module.getattr("flow_mapping_colon_plain_key").unwrap();
            let flow_mapping_colon_plain_key =
                extract_template(py, &flow_mapping_colon_plain_key, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_mapping_colon_plain_key).unwrap())
                    .unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            assert_string_entry(value, "a:b", "c");

            let flow_mapping_colon_and_hash =
                module.getattr("flow_mapping_colon_and_hash").unwrap();
            let flow_mapping_colon_and_hash =
                extract_template(py, &flow_mapping_colon_and_hash, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_mapping_colon_and_hash).unwrap())
                    .unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            assert_string_entry(value, "a:b", "c#d");

            let block_plain_colon_no_space = module.getattr("block_plain_colon_no_space").unwrap();
            let block_plain_colon_no_space =
                extract_template(py, &block_plain_colon_no_space, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&block_plain_colon_no_space).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_string_entry(&documents[0], "value", "a:b:c");

            let flow_plain_colon_hash_deeper =
                module.getattr("flow_plain_colon_hash_deeper").unwrap();
            let flow_plain_colon_hash_deeper =
                extract_template(py, &flow_plain_colon_hash_deeper, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_plain_colon_hash_deeper).unwrap())
                    .unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            match value {
                YamlOwned::Sequence(sequence) => {
                    assert_eq!(sequence.len(), 1);
                    assert_eq!(yaml_scalar_text(&sequence[0]), Some("a:b:c#d"));
                }
                _ => panic!("expected YAML sequence"),
            }

            let alias_in_flow_mapping_value =
                module.getattr("alias_in_flow_mapping_value").unwrap();
            let alias_in_flow_mapping_value =
                extract_template(py, &alias_in_flow_mapping_value, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&alias_in_flow_mapping_value).unwrap())
                    .unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            let reference = yaml_mapping_entry(value, "ref").expect("expected ref");
            assert_integer_entry(reference, "x", 1);

            let flow_null_and_alias = module.getattr("flow_null_and_alias").unwrap();
            let flow_null_and_alias =
                extract_template(py, &flow_null_and_alias, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_null_and_alias).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            let null_value = yaml_mapping(value)
                .and_then(|entries| {
                    entries
                        .iter()
                        .find_map(|(key, value)| key.is_null().then_some(value))
                })
                .expect("expected null key");
            assert_integer_entry(null_value, "x", 1);

            let alias_seq_value = module.getattr("alias_seq_value").unwrap();
            let alias_seq_value =
                extract_template(py, &alias_seq_value, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&alias_seq_value).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(
                yaml_sequence_len(yaml_mapping_entry(&documents[0], "a").expect("expected key a")),
                Some(2)
            );
            assert_eq!(
                yaml_sequence_len(yaml_mapping_entry(&documents[0], "b").expect("expected key b")),
                Some(2)
            );

            let flow_mapping_missing_value = module.getattr("flow_mapping_missing_value").unwrap();
            let flow_mapping_missing_value =
                extract_template(py, &flow_mapping_missing_value, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_mapping_missing_value).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            let a = yaml_mapping_entry(value, "a").expect("expected key a");
            assert!(a.is_null());

            let flow_seq_missing_value_before_end =
                module.getattr("flow_seq_missing_value_before_end").unwrap();
            let flow_seq_missing_value_before_end =
                extract_template(py, &flow_seq_missing_value_before_end, "yaml_t/yaml_t_str")
                    .unwrap();
            let rendered = render_document(
                py,
                &parse_template(&flow_seq_missing_value_before_end).unwrap(),
            )
            .unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            assert_eq!(yaml_sequence_len(value), Some(2));

            let flow_alias_map = module.getattr("flow_alias_map").unwrap();
            let flow_alias_map =
                extract_template(py, &flow_alias_map, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&flow_alias_map).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            assert_integer_entry(value, "left", 1);
            assert_integer_entry(value, "right", 1);

            let flow_alias_seq = module.getattr("flow_alias_seq").unwrap();
            let flow_alias_seq =
                extract_template(py, &flow_alias_seq, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&flow_alias_seq).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            assert_eq!(yaml_sequence_len(value), Some(2));

            let flow_merge = module.getattr("flow_merge").unwrap();
            let flow_merge = extract_template(py, &flow_merge, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&flow_merge).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert!(rendered.text.contains("<<: &base"));
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            assert_integer_entry(value, "b", 2);

            let template = module.getattr("nested_flow_alias_merge").unwrap();
            let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            let sequence = match value {
                YamlOwned::Sequence(sequence) => sequence,
                _ => panic!("expected YAML sequence"),
            };
            assert_eq!(sequence.len(), 2);
            assert_integer_entry(&sequence[0], "b", 2);
            assert_integer_entry(&sequence[1], "a", 1);

            let explicit_seq = module.getattr("explicit_seq").unwrap();
            let explicit_seq = extract_template(py, &explicit_seq, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&explicit_seq).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let mapping = documents[0].as_mapping().expect("expected YAML mapping");
            let value = mapping
                .iter()
                .find_map(|(key, value)| (yaml_scalar_text(key) == Some("a")).then_some(value))
                .expect("expected key a");
            assert_eq!(yaml_sequence_len(value), Some(2));

            let indented_block = module.getattr("indented_block").unwrap();
            let indented_block =
                extract_template(py, &indented_block, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&indented_block).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_string_entry(&documents[0], "value", "a\nb\n");
            assert_eq!(rendered.text, "value: |1\n a\n b\n");
        });
    }

    #[test]
    fn rejects_parse_render_and_validation_contracts() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nclass BadStringValue:\n    def __str__(self):\n        raise ValueError('cannot stringify')\nbad=BadStringValue()\ntag='bad tag'\nparse_open=t'value: [1, 2'\nparse_tab=Template('a:\\t1\\n')\nparse_nested_tab=Template('a:\\n  b:\\n\\t- 1\\n')\nparse_trailing=Template('value: *not alias\\n')\nparse_empty_flow=Template('[1,,2]\\n')\nparse_trailing_entry=Template('value: [1, 2,,]\\n')\nparse_empty_mapping=Template('value: {,}\\n')\nparse_missing_colon=Template('value: {a b}\\n')\nparse_extra_comma=Template('value: {a: 1,, b: 2}\\n')\nunknown_anchor=Template('value: *not_alias\\n')\ncross_doc_anchor=Template('--- &a\\n- 1\\n- 2\\n---\\n*a\\n')\nfragment_template=t'label: \"hi-{bad}\"'\nmetadata_template=t'value: !{tag} ok'\nfloat_template=t'value: {float(\"inf\")}'\n"
                ),
                pyo3::ffi::c_str!("test_yaml_error_contracts.py"),
                pyo3::ffi::c_str!("test_yaml_error_contracts"),
            )
            .unwrap();

            for (name, expected) in [
                ("parse_open", "Expected"),
                ("parse_tab", "Tabs are not allowed"),
                ("parse_nested_tab", "Tabs are not allowed"),
                ("parse_trailing", "Unexpected trailing YAML content"),
                ("parse_empty_flow", "Expected a YAML value"),
                ("parse_trailing_entry", "Expected"),
                ("parse_empty_mapping", "Expected ':' in YAML template"),
                ("parse_missing_colon", "Expected ':' in YAML template"),
                ("parse_extra_comma", "Expected ':' in YAML template"),
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
                let err = parse_template(&template).expect_err("expected YAML parse failure");
                assert_eq!(err.kind, ErrorKind::Parse);
                assert!(err.message.contains(expected), "{name}: {}", err.message);
            }

            let unknown_anchor = module.getattr("unknown_anchor").unwrap();
            let unknown_anchor =
                extract_template(py, &unknown_anchor, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&unknown_anchor).unwrap()).unwrap();
            let err = parse_rendered_yaml(&rendered.text)
                .expect_err("expected YAML unknown-anchor parse failure");
            assert_eq!(err.kind, ErrorKind::Parse);
            assert!(err.message.contains("unknown anchor"));

            let cross_doc_anchor = module.getattr("cross_doc_anchor").unwrap();
            let cross_doc_anchor =
                extract_template(py, &cross_doc_anchor, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&cross_doc_anchor).unwrap()).unwrap();
            let err = parse_rendered_yaml(&rendered.text)
                .expect_err("expected YAML cross-document anchor parse failure");
            assert_eq!(err.kind, ErrorKind::Parse);
            assert!(err.message.contains("unknown anchor"));

            for (name, expected) in [
                ("fragment_template", "fragment"),
                ("metadata_template", "metadata"),
                ("float_template", "non-finite float"),
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
                let document = parse_template(&template).unwrap();
                let err = match render_document(py, &document) {
                    Ok(_) => panic!("expected YAML render failure"),
                    Err(err) => err,
                };
                assert_eq!(err.kind, ErrorKind::Unrepresentable);
                assert!(err.message.contains(expected), "{name}: {}", err.message);
            }
        });
    }

    #[test]
    fn renders_custom_tag_stream_and_complex_key_text() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ncomment_only_tail_stream=Template('---\\na: 1\\n--- # comment\\n...\\n')\ncustom_tag_sequence=Template('value: !custom [1, 2]\\n')\nflow_alias_map=Template('value: {left: &a 1, right: *a}\\n')\n"
                ),
                pyo3::ffi::c_str!("test_yaml_render_streams_and_keys.py"),
                pyo3::ffi::c_str!("test_yaml_render_streams_and_keys"),
            )
            .unwrap();

            let comment_only_tail_stream = module.getattr("comment_only_tail_stream").unwrap();
            let comment_only_tail_stream =
                extract_template(py, &comment_only_tail_stream, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&comment_only_tail_stream).unwrap()).unwrap();
            assert_eq!(rendered.text, "---\na: 1\n---\nnull\n...");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(documents.len(), 2);
            assert_integer_entry(&documents[0], "a", 1);
            assert!(documents[1].is_null());

            let custom_tag_sequence = module.getattr("custom_tag_sequence").unwrap();
            let custom_tag_sequence =
                extract_template(py, &custom_tag_sequence, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&custom_tag_sequence).unwrap()).unwrap();
            assert_eq!(rendered.text, "value: !custom [ 1, 2 ]");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            assert_eq!(yaml_sequence_len(value), Some(2));

            let flow_alias_map = module.getattr("flow_alias_map").unwrap();
            let flow_alias_map =
                extract_template(py, &flow_alias_map, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&flow_alias_map).unwrap()).unwrap();
            assert_eq!(rendered.text, "value: { left: &a 1, right: *a }");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("expected value key");
            assert_integer_entry(value, "left", 1);
            assert_integer_entry(value, "right", 1);
        });
    }

    #[test]
    fn renders_custom_tag_scalar_mapping_and_root_sequence_shapes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "scalar_root=t'!custom 3\\n'\nmapping=t'value: !custom 3\\n'\nsequence=t'value: !custom [1, 2]\\n'\ncommented_root_sequence=t'--- # comment\\n!custom [1, 2]\\n'\n"
                ),
                pyo3::ffi::c_str!("test_yaml_custom_tag_shapes.py"),
                pyo3::ffi::c_str!("test_yaml_custom_tag_shapes"),
            )
            .unwrap();

            let scalar_root = module.getattr("scalar_root").unwrap();
            let scalar_root = extract_template(py, &scalar_root, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&scalar_root).unwrap()).unwrap();
            assert_eq!(rendered.text, "!custom 3");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(yaml_integer(&documents[0]), Some(3));

            let mapping = module.getattr("mapping").unwrap();
            let mapping = extract_template(py, &mapping, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&mapping).unwrap()).unwrap();
            assert_eq!(rendered.text, "value: !custom 3");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_integer_entry(&documents[0], "value", 3);

            let sequence = module.getattr("sequence").unwrap();
            let sequence = extract_template(py, &sequence, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&sequence).unwrap()).unwrap();
            assert_eq!(rendered.text, "value: !custom [ 1, 2 ]");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(
                yaml_sequence_len(yaml_mapping_entry(&documents[0], "value").expect("value key")),
                Some(2)
            );

            let commented_root_sequence = module.getattr("commented_root_sequence").unwrap();
            let commented_root_sequence =
                extract_template(py, &commented_root_sequence, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&commented_root_sequence).unwrap()).unwrap();
            assert_eq!(rendered.text, "---\n!custom [ 1, 2 ]");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(yaml_sequence_len(&documents[0]), Some(2));
        });
    }

    #[test]
    fn renders_core_schema_scalars_and_top_level_sequence() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ncore_scalars=Template('value: true\\nnone: null\\nlegacy_bool: on\\nlegacy_yes: yes\\n')\ntop_level_sequence=Template('- 1\\n- true\\n- null\\n- on\\n')\n"
                ),
                pyo3::ffi::c_str!("test_yaml_core_scalars_and_sequence.py"),
                pyo3::ffi::c_str!("test_yaml_core_scalars_and_sequence"),
            )
            .unwrap();

            let core_scalars = module.getattr("core_scalars").unwrap();
            let core_scalars = extract_template(py, &core_scalars, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&core_scalars).unwrap()).unwrap();
            assert_eq!(
                rendered.text,
                "value: true\nnone: null\nlegacy_bool: on\nlegacy_yes: yes"
            );
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(documents.len(), 1);
            assert_eq!(
                yaml_mapping_entry(&documents[0], "value").and_then(YamlOwned::as_bool),
                Some(true)
            );
            assert!(
                yaml_mapping_entry(&documents[0], "none")
                    .expect("none key")
                    .is_null()
            );
            assert_string_entry(&documents[0], "legacy_bool", "on");
            assert_string_entry(&documents[0], "legacy_yes", "yes");

            let top_level_sequence = module.getattr("top_level_sequence").unwrap();
            let top_level_sequence =
                extract_template(py, &top_level_sequence, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&top_level_sequence).unwrap()).unwrap();
            assert_eq!(rendered.text, "- 1\n- true\n- null\n- on");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(documents.len(), 1);
            let sequence = documents[0].as_vec().expect("expected top-level sequence");
            assert_eq!(sequence.len(), 4);
            assert_eq!(yaml_integer(&sequence[0]), Some(1));
            assert_eq!(sequence[1].as_bool(), Some(true));
            assert!(sequence[2].is_null());
            assert_eq!(yaml_scalar_text(&sequence[3]), Some("on"));
        });
    }

    #[test]
    fn renders_end_to_end_supported_positions_text_and_data() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "user='Alice'\nkey='owner'\nanchor='item'\ntag='str'\ntemplate=t'''\n{key}: {user}\nlabel: \"prefix-{user}\"\nplain: item-{user}\nitems:\n  - &{anchor} {user}\n  - *{anchor}\ntagged: !{tag} {user}\nflow: [{user}, {{label: {user}}}]\n'''\n"
                ),
                pyo3::ffi::c_str!("test_yaml_end_to_end_positions.py"),
                pyo3::ffi::c_str!("test_yaml_end_to_end_positions"),
            )
            .unwrap();

            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
            assert_eq!(
                rendered.text,
                "\"owner\": \"Alice\"\nlabel: \"prefix-Alice\"\nplain: item-Alice\nitems:\n  - &item \"Alice\"\n  - *item\ntagged: !str \"Alice\"\nflow: [ \"Alice\", { label: \"Alice\" } ]"
            );
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(documents.len(), 1);
            assert_string_entry(&documents[0], "owner", "Alice");
            assert_string_entry(&documents[0], "label", "prefix-Alice");
            assert_string_entry(&documents[0], "plain", "item-Alice");
            assert_string_entry(&documents[0], "tagged", "Alice");
            let items = yaml_mapping_entry(&documents[0], "items").expect("items key");
            let items = items.as_vec().expect("items sequence");
            assert_eq!(items.len(), 2);
            assert_eq!(yaml_scalar_text(&items[0]), Some("Alice"));
            assert_eq!(yaml_scalar_text(&items[1]), Some("Alice"));
            let flow = yaml_mapping_entry(&documents[0], "flow").expect("flow key");
            let flow = flow.as_vec().expect("flow sequence");
            assert_eq!(yaml_scalar_text(&flow[0]), Some("Alice"));
            assert_string_entry(&flow[1], "label", "Alice");
        });
    }

    #[test]
    fn renders_block_scalars_and_sequence_item_text_and_data() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "user='Alice'\ntemplate=t'''\nliteral: |\n  hello {user}\n  world\nfolded: >\n  hello {user}\n  world\nlines:\n  - |\n      item {user}\n'''\n"
                ),
                pyo3::ffi::c_str!("test_yaml_block_scalars_render.py"),
                pyo3::ffi::c_str!("test_yaml_block_scalars_render"),
            )
            .unwrap();

            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
            assert_eq!(
                rendered.text,
                "literal: |\n  hello Alice\n  world\nfolded: >\n  hello Alice\n  world\nlines:\n  - |\n    item Alice\n"
            );
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(documents.len(), 1);
            assert_string_entry(&documents[0], "literal", "hello Alice\nworld\n");
            assert_string_entry(&documents[0], "folded", "hello Alice world\n");
            let lines = yaml_mapping_entry(&documents[0], "lines").expect("lines key");
            let lines = lines.as_vec().expect("lines sequence");
            assert_eq!(yaml_scalar_text(&lines[0]), Some("item Alice\n"));
        });
    }

    #[test]
    fn renders_comment_only_document_variants_and_mid_stream_shapes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "comment_only=t'# comment\\n'\ncomment_only_explicit=t'--- # comment\\n'\ncomment_only_explicit_end=t'--- # comment\\n...\\n'\ncomment_only_mid_stream=t'---\\na: 1\\n--- # comment\\n...\\n---\\nb: 2\\n'\n"
                ),
                pyo3::ffi::c_str!("test_yaml_comment_variants.py"),
                pyo3::ffi::c_str!("test_yaml_comment_variants"),
            )
            .unwrap();

            for (name, expected_text) in [
                ("comment_only", "null"),
                ("comment_only_explicit", "---\nnull"),
                ("comment_only_explicit_end", "---\nnull\n..."),
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
                let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
                assert_eq!(rendered.text, expected_text);
                let documents = parse_rendered_yaml(&rendered.text).unwrap();
                assert_eq!(documents.len(), 1);
                assert!(documents[0].is_null());
            }

            let comment_only_mid_stream = module.getattr("comment_only_mid_stream").unwrap();
            let comment_only_mid_stream =
                extract_template(py, &comment_only_mid_stream, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&comment_only_mid_stream).unwrap()).unwrap();
            assert_eq!(rendered.text, "---\na: 1\n---\nnull\n...\n---\nb: 2");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(documents.len(), 3);
            assert_integer_entry(&documents[0], "a", 1);
            assert!(documents[1].is_null());
            assert_integer_entry(&documents[2], "b", 2);
        });
    }

    #[test]
    fn renders_tag_directives_and_handle_tag_roots() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "tag_directive_scalar=t'%TAG !e! tag:example.com,2020:\\n---\\nvalue: !e!foo 1\\n'\ntag_directive_root=t'%YAML 1.2\\n%TAG !e! tag:example.com,2020:\\n---\\n!e!root {{value: !e!leaf 1}}\\n'\ntag_directive_root_comment=t'%YAML 1.2\\n%TAG !e! tag:example.com,2020:\\n--- # comment\\n!e!root {{value: !e!leaf 1}}\\n'\nverbatim_root_mapping=t'--- !<tag:yaml.org,2002:map>\\na: 1\\n'\nverbatim_root_sequence=t'--- !<tag:yaml.org,2002:seq>\\n- 1\\n- 2\\n'\n"
                ),
                pyo3::ffi::c_str!("test_yaml_tag_directives.py"),
                pyo3::ffi::c_str!("test_yaml_tag_directives"),
            )
            .unwrap();

            let tag_directive_scalar = module.getattr("tag_directive_scalar").unwrap();
            let tag_directive_scalar =
                extract_template(py, &tag_directive_scalar, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&tag_directive_scalar).unwrap()).unwrap();
            assert_eq!(
                rendered.text,
                "%TAG !e! tag:example.com,2020:\n---\nvalue: !e!foo 1"
            );
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_integer_entry(&documents[0], "value", 1);

            let tag_directive_root = module.getattr("tag_directive_root").unwrap();
            let tag_directive_root =
                extract_template(py, &tag_directive_root, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&tag_directive_root).unwrap()).unwrap();
            assert_eq!(
                rendered.text,
                "%YAML 1.2\n%TAG !e! tag:example.com,2020:\n---\n!e!root { value: !e!leaf 1 }"
            );
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_integer_entry(&documents[0], "value", 1);

            let tag_directive_root_comment = module.getattr("tag_directive_root_comment").unwrap();
            let tag_directive_root_comment =
                extract_template(py, &tag_directive_root_comment, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&tag_directive_root_comment).unwrap()).unwrap();
            assert_eq!(
                rendered.text,
                "%YAML 1.2\n%TAG !e! tag:example.com,2020:\n---\n!e!root { value: !e!leaf 1 }"
            );
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_integer_entry(&documents[0], "value", 1);

            let verbatim_root_mapping = module.getattr("verbatim_root_mapping").unwrap();
            let verbatim_root_mapping =
                extract_template(py, &verbatim_root_mapping, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&verbatim_root_mapping).unwrap()).unwrap();
            assert_eq!(rendered.text, "---\n!<tag:yaml.org,2002:map>\na: 1");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_integer_entry(&documents[0], "a", 1);

            let verbatim_root_sequence = module.getattr("verbatim_root_sequence").unwrap();
            let verbatim_root_sequence =
                extract_template(py, &verbatim_root_sequence, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&verbatim_root_sequence).unwrap()).unwrap();
            assert_eq!(rendered.text, "---\n!<tag:yaml.org,2002:seq>\n- 1\n- 2");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(yaml_sequence_len(&documents[0]), Some(2));
        });
    }

    #[test]
    fn renders_explicit_core_tag_root_shapes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "root_bool=t'--- !!bool true\\n'\nroot_str=t'--- !!str true\\n'\nroot_int=t'--- !!int 3\\n'\n"
                ),
                pyo3::ffi::c_str!("test_yaml_core_tag_roots.py"),
                pyo3::ffi::c_str!("test_yaml_core_tag_roots"),
            )
            .unwrap();

            for (name, expected_text) in [
                ("root_bool", "---\n!!bool true"),
                ("root_str", "---\n!!str true"),
                ("root_int", "---\n!!int 3"),
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
                let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
                assert_eq!(rendered.text, expected_text);
            }

            let root_bool = module.getattr("root_bool").unwrap();
            let root_bool = extract_template(py, &root_bool, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&root_bool).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(documents[0].as_bool(), Some(true));

            let root_str = module.getattr("root_str").unwrap();
            let root_str = extract_template(py, &root_str, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&root_str).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(yaml_scalar_text(&documents[0]), Some("true"));

            let root_int = module.getattr("root_int").unwrap();
            let root_int = extract_template(py, &root_int, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&root_int).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(yaml_integer(&documents[0]), Some(3));
        });
    }

    #[test]
    fn renders_explicit_core_tag_mapping_and_root_text_families() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "mapping=t'value_bool: !!bool true\\nvalue_str: !!str true\\nvalue_float: !!float 1\\nvalue_null: !!null null\\n'\nroot_int=t'--- !!int 3\\n'\nroot_str=t'--- !!str true\\n'\nroot_bool=t'--- !!bool true\\n'\n"
                ),
                pyo3::ffi::c_str!("test_yaml_explicit_core_tag_families.py"),
                pyo3::ffi::c_str!("test_yaml_explicit_core_tag_families"),
            )
            .unwrap();

            let mapping = module.getattr("mapping").unwrap();
            let mapping = extract_template(py, &mapping, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&mapping).unwrap()).unwrap();
            assert_eq!(
                rendered.text,
                "value_bool: !!bool true\nvalue_str: !!str true\nvalue_float: !!float 1\nvalue_null: !!null null"
            );
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(
                yaml_mapping_entry(&documents[0], "value_bool").and_then(YamlOwned::as_bool),
                Some(true)
            );
            assert_eq!(
                yaml_scalar_text(
                    yaml_mapping_entry(&documents[0], "value_str").expect("value_str")
                ),
                Some("true")
            );
            assert_eq!(
                yaml_mapping_entry(&documents[0], "value_float").and_then(yaml_float),
                Some(1.0)
            );
            assert!(
                yaml_mapping_entry(&documents[0], "value_null")
                    .expect("value_null")
                    .is_null()
            );

            for (name, expected_text) in [
                ("root_int", "---\n!!int 3"),
                ("root_str", "---\n!!str true"),
                ("root_bool", "---\n!!bool true"),
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
                let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
                assert_eq!(rendered.text, expected_text, "{name}");
            }
        });
    }

    #[test]
    fn renders_flow_trailing_comma_explicit_key_and_indent_indicator_families() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nflow_sequence=t'[1, 2,]\\n'\nflow_mapping=Template('{a: 1,}\\n')\nexplicit_key_sequence_value=t'? a\\n: - 1\\n  - 2\\n'\nindent_indicator=t'value: |1\\n a\\n b\\n'\n"
                ),
                pyo3::ffi::c_str!("test_yaml_flow_indent_families.py"),
                pyo3::ffi::c_str!("test_yaml_flow_indent_families"),
            )
            .unwrap();

            let flow_sequence = module.getattr("flow_sequence").unwrap();
            let flow_sequence = extract_template(py, &flow_sequence, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&flow_sequence).unwrap()).unwrap();
            assert_eq!(rendered.text, "[ 1, 2 ]");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(yaml_sequence_len(&documents[0]), Some(2));

            let flow_mapping = module.getattr("flow_mapping").unwrap();
            let flow_mapping = extract_template(py, &flow_mapping, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&flow_mapping).unwrap()).unwrap();
            assert_eq!(rendered.text, "{ a: 1 }");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_integer_entry(&documents[0], "a", 1);

            let explicit_key_sequence_value =
                module.getattr("explicit_key_sequence_value").unwrap();
            let explicit_key_sequence_value =
                extract_template(py, &explicit_key_sequence_value, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&explicit_key_sequence_value).unwrap())
                    .unwrap();
            assert_eq!(rendered.text, "? a\n:\n  - 1\n  - 2");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let entry = documents[0].as_mapping().expect("mapping");
            assert_eq!(entry.len(), 1);

            let indent_indicator = module.getattr("indent_indicator").unwrap();
            let indent_indicator =
                extract_template(py, &indent_indicator, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&indent_indicator).unwrap()).unwrap();
            assert_eq!(rendered.text, "value: |1\n a\n b\n");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_string_entry(&documents[0], "value", "a\nb\n");
        });
    }

    #[test]
    fn renders_flow_alias_and_merge_shapes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nflow_alias_map=Template('value: {left: &a 1, right: *a}\\n')\nflow_alias_seq=Template('value: [&a 1, *a]\\n')\nflow_merge=Template('value: {<<: &base {a: 1}, b: 2}\\n')\n"
                ),
                pyo3::ffi::c_str!("test_yaml_flow_alias_merge.py"),
                pyo3::ffi::c_str!("test_yaml_flow_alias_merge"),
            )
            .unwrap();

            let flow_alias_map = module.getattr("flow_alias_map").unwrap();
            let flow_alias_map =
                extract_template(py, &flow_alias_map, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&flow_alias_map).unwrap()).unwrap();
            assert_eq!(rendered.text, "value: { left: &a 1, right: *a }");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("value key");
            assert_integer_entry(value, "left", 1);
            assert_integer_entry(value, "right", 1);

            let flow_alias_seq = module.getattr("flow_alias_seq").unwrap();
            let flow_alias_seq =
                extract_template(py, &flow_alias_seq, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&flow_alias_seq).unwrap()).unwrap();
            assert_eq!(rendered.text, "value: [ &a 1, *a ]");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("value key");
            let value = value.as_vec().expect("value seq");
            assert_eq!(yaml_integer(&value[0]), Some(1));
            assert_eq!(yaml_integer(&value[1]), Some(1));

            let flow_merge = module.getattr("flow_merge").unwrap();
            let flow_merge = extract_template(py, &flow_merge, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&flow_merge).unwrap()).unwrap();
            assert_eq!(rendered.text, "value: { <<: &base { a: 1 }, b: 2 }");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("value key");
            assert_integer_entry(value, "b", 2);
            assert!(
                yaml_mapping_entry(value, "a").is_some()
                    || yaml_mapping_entry(value, "<<").is_some()
            );
        });
    }

    #[test]
    fn renders_document_stream_and_root_decorator_shapes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ncomment_only_explicit_end_document=Template('--- # comment\\n...\\n')\ncomment_only_explicit_end_stream=Template('--- # comment\\n...\\n---\\na: 1\\n')\ncomment_only_mid_stream=Template('---\\na: 1\\n--- # comment\\n...\\n---\\nb: 2\\n')\nexplicit_end_comment_stream=Template('---\\na: 1\\n... # end\\n---\\nb: 2\\n')\ndoc_start_comment=Template('--- # comment\\nvalue: 1\\n')\ndoc_start_tag_comment=Template('--- !!str true # comment\\n')\ntagged_block_root_mapping=Template('--- !!map\\na: 1\\n')\ntagged_block_root_sequence=Template('--- !!seq\\n- 1\\n- 2\\n')\nroot_anchor_sequence=Template('--- &root\\n  - 1\\n  - 2\\n')\nroot_anchor_custom_mapping=Template('--- &root !custom\\n  a: 1\\n')\nroot_custom_anchor_sequence=Template('--- !custom &root\\n  - 1\\n  - 2\\n')\nflow_newline=Template('{a: 1, b: [2, 3]}\\n')\n"
                ),
                pyo3::ffi::c_str!("test_yaml_stream_root_shapes.py"),
                pyo3::ffi::c_str!("test_yaml_stream_root_shapes"),
            )
            .unwrap();

            let comment_only_explicit_end_document = module
                .getattr("comment_only_explicit_end_document")
                .unwrap();
            let comment_only_explicit_end_document =
                extract_template(py, &comment_only_explicit_end_document, "yaml_t/yaml_t_str")
                    .unwrap();
            let rendered = render_document(
                py,
                &parse_template(&comment_only_explicit_end_document).unwrap(),
            )
            .unwrap();
            assert_eq!(rendered.text, "---\nnull\n...");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert!(matches!(documents[0], YamlOwned::Value(_)));

            let comment_only_explicit_end_stream =
                module.getattr("comment_only_explicit_end_stream").unwrap();
            let comment_only_explicit_end_stream =
                extract_template(py, &comment_only_explicit_end_stream, "yaml_t/yaml_t_str")
                    .unwrap();
            let rendered = render_document(
                py,
                &parse_template(&comment_only_explicit_end_stream).unwrap(),
            )
            .unwrap();
            assert_eq!(rendered.text, "---\nnull\n...\n---\na: 1");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert!(matches!(documents[0], YamlOwned::Value(_)));
            assert_integer_entry(&documents[1], "a", 1);

            let comment_only_mid_stream = module.getattr("comment_only_mid_stream").unwrap();
            let comment_only_mid_stream =
                extract_template(py, &comment_only_mid_stream, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&comment_only_mid_stream).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_integer_entry(&documents[0], "a", 1);
            assert!(matches!(documents[1], YamlOwned::Value(_)));
            assert_integer_entry(&documents[2], "b", 2);

            let explicit_end_comment_stream =
                module.getattr("explicit_end_comment_stream").unwrap();
            let explicit_end_comment_stream =
                extract_template(py, &explicit_end_comment_stream, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&explicit_end_comment_stream).unwrap())
                    .unwrap();
            assert_eq!(rendered.text, "---\na: 1\n...\n---\nb: 2");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_integer_entry(&documents[0], "a", 1);
            assert_integer_entry(&documents[1], "b", 2);

            let doc_start_comment = module.getattr("doc_start_comment").unwrap();
            let doc_start_comment =
                extract_template(py, &doc_start_comment, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&doc_start_comment).unwrap()).unwrap();
            assert_eq!(rendered.text, "---\nvalue: 1");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_integer_entry(&documents[0], "value", 1);

            let doc_start_tag_comment = module.getattr("doc_start_tag_comment").unwrap();
            let doc_start_tag_comment =
                extract_template(py, &doc_start_tag_comment, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&doc_start_tag_comment).unwrap()).unwrap();
            assert_eq!(rendered.text, "---\n!!str true");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(yaml_scalar_text(&documents[0]), Some("true"));

            let tagged_block_root_mapping = module.getattr("tagged_block_root_mapping").unwrap();
            let tagged_block_root_mapping =
                extract_template(py, &tagged_block_root_mapping, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&tagged_block_root_mapping).unwrap()).unwrap();
            assert_eq!(rendered.text, "---\n!!map\na: 1");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_integer_entry(&documents[0], "a", 1);

            let tagged_block_root_sequence = module.getattr("tagged_block_root_sequence").unwrap();
            let tagged_block_root_sequence =
                extract_template(py, &tagged_block_root_sequence, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&tagged_block_root_sequence).unwrap()).unwrap();
            assert_eq!(rendered.text, "---\n!!seq\n- 1\n- 2");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(documents[0].as_vec().expect("root seq").len(), 2);

            let root_anchor_sequence = module.getattr("root_anchor_sequence").unwrap();
            let root_anchor_sequence =
                extract_template(py, &root_anchor_sequence, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&root_anchor_sequence).unwrap()).unwrap();
            assert_eq!(rendered.text, "---\n&root\n- 1\n- 2");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(yaml_sequence_len(&documents[0]), Some(2));

            let root_anchor_custom_mapping = module.getattr("root_anchor_custom_mapping").unwrap();
            let root_anchor_custom_mapping =
                extract_template(py, &root_anchor_custom_mapping, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&root_anchor_custom_mapping).unwrap()).unwrap();
            assert_eq!(rendered.text, "---\n!custom &root\na: 1");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_integer_entry(&documents[0], "a", 1);

            let root_custom_anchor_sequence =
                module.getattr("root_custom_anchor_sequence").unwrap();
            let root_custom_anchor_sequence =
                extract_template(py, &root_custom_anchor_sequence, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&root_custom_anchor_sequence).unwrap())
                    .unwrap();
            assert_eq!(rendered.text, "---\n!custom &root\n- 1\n- 2");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(yaml_sequence_len(&documents[0]), Some(2));

            let flow_newline = module.getattr("flow_newline").unwrap();
            let flow_newline = extract_template(py, &flow_newline, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&flow_newline).unwrap()).unwrap();
            assert_eq!(rendered.text, "{ a: 1, b: [ 2, 3 ] }");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let mapping = yaml_mapping(&documents[0]).expect("expected root flow mapping");
            assert_eq!(mapping.len(), 2);
        });
    }

    #[test]
    fn renders_merge_and_collection_shape_families() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nmerge=Template('base: &base\\n  a: 1\\n  b: 2\\nderived:\\n  <<: *base\\n  c: 3\\n')\nflow_nested_alias_merge=Template('value: [{<<: &base {a: 1}, b: 2}, *base]\\n')\nalias_seq_value=Template('a: &x [1, 2]\\nb: *x\\n')\nempty_flow_sequence=Template('value: []\\n')\nempty_flow_mapping=Template('value: {}\\n')\nflow_mapping_missing_value=Template('value: {a: }\\n')\nflow_seq_missing_value_before_end=Template('value: [1, 2, ]\\n')\nindentless_sequence_value=Template('a:\\n- 1\\n- 2\\n')\nsequence_of_mappings=Template('- a: 1\\n  b: 2\\n- c: 3\\n')\nmapping_of_sequence_of_mappings=Template('items:\\n- a: 1\\n  b: 2\\n- c: 3\\n')\nsequence_of_sequences=Template('- - 1\\n  - 2\\n- - 3\\n')\n"
                ),
                pyo3::ffi::c_str!("test_yaml_merge_collection_shapes.py"),
                pyo3::ffi::c_str!("test_yaml_merge_collection_shapes"),
            )
            .unwrap();

            let merge = module.getattr("merge").unwrap();
            let merge = extract_template(py, &merge, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&merge).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let derived = yaml_mapping_entry(&documents[0], "derived").expect("derived");
            assert_integer_entry(derived, "c", 3);
            assert!(
                yaml_mapping_entry(derived, "a").is_some()
                    || yaml_mapping_entry(derived, "<<").is_some()
            );
            assert!(
                yaml_mapping_entry(derived, "b").is_some()
                    || yaml_mapping_entry(derived, "<<").is_some()
            );

            let flow_nested_alias_merge = module.getattr("flow_nested_alias_merge").unwrap();
            let flow_nested_alias_merge =
                extract_template(py, &flow_nested_alias_merge, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_nested_alias_merge).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("value key");
            let value = value.as_vec().expect("value seq");
            assert_eq!(value.len(), 2);

            let alias_seq_value = module.getattr("alias_seq_value").unwrap();
            let alias_seq_value =
                extract_template(py, &alias_seq_value, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&alias_seq_value).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(
                yaml_sequence_len(yaml_mapping_entry(&documents[0], "a").expect("a")),
                Some(2)
            );
            assert_eq!(
                yaml_sequence_len(yaml_mapping_entry(&documents[0], "b").expect("b")),
                Some(2)
            );

            for (name, expected_len) in [
                ("empty_flow_sequence", 0usize),
                ("flow_seq_missing_value_before_end", 2usize),
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
                let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
                let documents = parse_rendered_yaml(&rendered.text).unwrap();
                let value = yaml_mapping_entry(&documents[0], "value").expect("value key");
                assert_eq!(yaml_sequence_len(value), Some(expected_len), "{name}");
            }

            let empty_flow_mapping = module.getattr("empty_flow_mapping").unwrap();
            let empty_flow_mapping =
                extract_template(py, &empty_flow_mapping, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&empty_flow_mapping).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("value key");
            assert_eq!(yaml_mapping(value).expect("mapping").len(), 0);

            let flow_mapping_missing_value = module.getattr("flow_mapping_missing_value").unwrap();
            let flow_mapping_missing_value =
                extract_template(py, &flow_mapping_missing_value, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_mapping_missing_value).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("value key");
            assert!(yaml_mapping_entry(value, "a").is_some());

            let indentless_sequence_value = module.getattr("indentless_sequence_value").unwrap();
            let indentless_sequence_value =
                extract_template(py, &indentless_sequence_value, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&indentless_sequence_value).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(
                yaml_sequence_len(yaml_mapping_entry(&documents[0], "a").expect("a")),
                Some(2)
            );

            let sequence_of_mappings = module.getattr("sequence_of_mappings").unwrap();
            let sequence_of_mappings =
                extract_template(py, &sequence_of_mappings, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&sequence_of_mappings).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(documents[0].as_vec().expect("top-level seq").len(), 2);

            let mapping_of_sequence_of_mappings =
                module.getattr("mapping_of_sequence_of_mappings").unwrap();
            let mapping_of_sequence_of_mappings =
                extract_template(py, &mapping_of_sequence_of_mappings, "yaml_t/yaml_t_str")
                    .unwrap();
            let rendered = render_document(
                py,
                &parse_template(&mapping_of_sequence_of_mappings).unwrap(),
            )
            .unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(
                yaml_sequence_len(yaml_mapping_entry(&documents[0], "items").expect("items")),
                Some(2)
            );

            let sequence_of_sequences = module.getattr("sequence_of_sequences").unwrap();
            let sequence_of_sequences =
                extract_template(py, &sequence_of_sequences, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&sequence_of_sequences).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let top = documents[0].as_vec().expect("top-level seq");
            assert_eq!(top.len(), 2);
            assert_eq!(yaml_sequence_len(&top[0]), Some(2));
            assert_eq!(yaml_sequence_len(&top[1]), Some(1));
        });
    }

    #[test]
    fn renders_flow_scalar_key_and_comment_edge_families() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nflow_plain_scalar_with_space=Template('value: [1 2]\\n')\nmapping_empty_flow_values=Template('value: {a: [], b: {}}\\n')\nflow_mapping_empty_key_and_values=Template('{\"\": [], foo: {}}\\n')\nflow_null_key=Template('{null: 1, \"\": 2}\\n')\nblock_null_key=Template('? null\\n: 1\\n')\nquoted_null_key=Template('? \"\"\\n: 1\\n')\nplain_question_mark_scalar=Template('value: ?x\\n')\nplain_colon_scalar_flow=Template('value: [a:b, c:d]\\n')\nflow_mapping_plain_key_questions=Template('value: {?x: 1, ?y: 2}\\n')\nflow_hash_mapping_four=Template('value: {a: b#c, d: e#f, g: h#i, j: k#l}\\n')\nflow_hash_seq_seven=Template('value: [a#b, c#d, e#f, g#h, i#j, k#l, m#n]\\n')\ncomment_after_flow_plain_colon=Template('value: [a:b # c\\n]\\n')\nflow_plain_comment_after_colon_deeper=Template('value: [a:b:c:d # comment\\n]\\n')\n"
                ),
                pyo3::ffi::c_str!("test_yaml_flow_scalar_edge_families.py"),
                pyo3::ffi::c_str!("test_yaml_flow_scalar_edge_families"),
            )
            .unwrap();

            let flow_plain_scalar_with_space =
                module.getattr("flow_plain_scalar_with_space").unwrap();
            let flow_plain_scalar_with_space =
                extract_template(py, &flow_plain_scalar_with_space, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_plain_scalar_with_space).unwrap())
                    .unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("value key");
            let value = value.as_vec().expect("value seq");
            assert_eq!(yaml_scalar_text(&value[0]), Some("1 2"));

            let mapping_empty_flow_values = module.getattr("mapping_empty_flow_values").unwrap();
            let mapping_empty_flow_values =
                extract_template(py, &mapping_empty_flow_values, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&mapping_empty_flow_values).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("value key");
            assert_eq!(
                yaml_sequence_len(yaml_mapping_entry(value, "a").expect("a")),
                Some(0)
            );
            assert_eq!(
                yaml_mapping(yaml_mapping_entry(value, "b").expect("b"))
                    .expect("b mapping")
                    .len(),
                0
            );

            let flow_mapping_empty_key_and_values =
                module.getattr("flow_mapping_empty_key_and_values").unwrap();
            let flow_mapping_empty_key_and_values =
                extract_template(py, &flow_mapping_empty_key_and_values, "yaml_t/yaml_t_str")
                    .unwrap();
            let rendered = render_document(
                py,
                &parse_template(&flow_mapping_empty_key_and_values).unwrap(),
            )
            .unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(
                yaml_sequence_len(yaml_mapping_entry(&documents[0], "").expect("empty key")),
                Some(0)
            );
            assert_eq!(
                yaml_mapping(yaml_mapping_entry(&documents[0], "foo").expect("foo"))
                    .expect("foo mapping")
                    .len(),
                0
            );

            let flow_null_key = module.getattr("flow_null_key").unwrap();
            let flow_null_key = extract_template(py, &flow_null_key, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&flow_null_key).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(documents[0].as_mapping().expect("mapping").len(), 2);

            let block_null_key = module.getattr("block_null_key").unwrap();
            let block_null_key =
                extract_template(py, &block_null_key, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&block_null_key).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(documents[0].as_mapping().expect("mapping").len(), 1);

            let quoted_null_key = module.getattr("quoted_null_key").unwrap();
            let quoted_null_key =
                extract_template(py, &quoted_null_key, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&quoted_null_key).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(documents[0].as_mapping().expect("mapping").len(), 1);

            let plain_question_mark_scalar = module.getattr("plain_question_mark_scalar").unwrap();
            let plain_question_mark_scalar =
                extract_template(py, &plain_question_mark_scalar, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&plain_question_mark_scalar).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_string_entry(&documents[0], "value", "?x");

            let plain_colon_scalar_flow = module.getattr("plain_colon_scalar_flow").unwrap();
            let plain_colon_scalar_flow =
                extract_template(py, &plain_colon_scalar_flow, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&plain_colon_scalar_flow).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("value key");
            let value = value.as_vec().expect("value seq");
            assert_eq!(yaml_scalar_text(&value[0]), Some("a:b"));
            assert_eq!(yaml_scalar_text(&value[1]), Some("c:d"));

            let flow_mapping_plain_key_questions =
                module.getattr("flow_mapping_plain_key_questions").unwrap();
            let flow_mapping_plain_key_questions =
                extract_template(py, &flow_mapping_plain_key_questions, "yaml_t/yaml_t_str")
                    .unwrap();
            let rendered = render_document(
                py,
                &parse_template(&flow_mapping_plain_key_questions).unwrap(),
            )
            .unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("value key");
            assert_eq!(yaml_mapping(value).expect("mapping").len(), 2);

            let flow_hash_mapping_four = module.getattr("flow_hash_mapping_four").unwrap();
            let flow_hash_mapping_four =
                extract_template(py, &flow_hash_mapping_four, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_hash_mapping_four).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("value key");
            assert_eq!(yaml_mapping(value).expect("mapping").len(), 4);

            let flow_hash_seq_seven = module.getattr("flow_hash_seq_seven").unwrap();
            let flow_hash_seq_seven =
                extract_template(py, &flow_hash_seq_seven, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_hash_seq_seven).unwrap()).unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("value key");
            assert_eq!(yaml_sequence_len(value), Some(7));

            let comment_after_flow_plain_colon =
                module.getattr("comment_after_flow_plain_colon").unwrap();
            let comment_after_flow_plain_colon =
                extract_template(py, &comment_after_flow_plain_colon, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(
                py,
                &parse_template(&comment_after_flow_plain_colon).unwrap(),
            )
            .unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("value key");
            let value = value.as_vec().expect("value seq");
            assert_eq!(yaml_scalar_text(&value[0]), Some("a:b"));

            let flow_plain_comment_after_colon_deeper = module
                .getattr("flow_plain_comment_after_colon_deeper")
                .unwrap();
            let flow_plain_comment_after_colon_deeper = extract_template(
                py,
                &flow_plain_comment_after_colon_deeper,
                "yaml_t/yaml_t_str",
            )
            .unwrap();
            let rendered = render_document(
                py,
                &parse_template(&flow_plain_comment_after_colon_deeper).unwrap(),
            )
            .unwrap();
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("value key");
            let value = value.as_vec().expect("value seq");
            assert_eq!(yaml_scalar_text(&value[0]), Some("a:b:c:d"));
        });
    }

    #[test]
    fn renders_flow_collection_comment_and_verbatim_tag_families() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nverbatim_tag=Template('value: !<tag:yaml.org,2002:str> hello\\n')\nflow_wrapped_sequence=Template('key: [a,\\n  b]\\n')\nflow_wrapped_mapping=Template('key: {a: 1,\\n  b: 2}\\n')\nflow_sequence_comment=Template('key: [a, # first\\n  b]\\n')\nflow_mapping_comment=Template('key: {a: 1, # first\\n  b: 2}\\n')\nalias_in_flow_mapping_value=Template('base: &a {x: 1}\\nvalue: {ref: *a}\\n')\nflow_null_and_alias=Template('base: &a {x: 1}\\nvalue: {null: *a}\\n')\n"
                ),
                pyo3::ffi::c_str!("test_yaml_flow_collection_comment_and_tag_families.py"),
                pyo3::ffi::c_str!("test_yaml_flow_collection_comment_and_tag_families"),
            )
            .unwrap();

            let verbatim_tag = module.getattr("verbatim_tag").unwrap();
            let verbatim_tag = extract_template(py, &verbatim_tag, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&verbatim_tag).unwrap()).unwrap();
            assert_eq!(rendered.text, "value: !<tag:yaml.org,2002:str> hello");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_string_entry(&documents[0], "value", "hello");

            for (name, expected_text) in [
                ("flow_wrapped_sequence", "key: [ a, b ]"),
                ("flow_sequence_comment", "key: [ a, b ]"),
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
                let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
                assert_eq!(rendered.text, expected_text, "{name}");
                let documents = parse_rendered_yaml(&rendered.text).unwrap();
                let value = yaml_mapping_entry(&documents[0], "key").expect("sequence key");
                assert_eq!(yaml_sequence_len(value), Some(2), "{name}");
            }

            for (name, expected_text) in [
                ("flow_wrapped_mapping", "key: { a: 1, b: 2 }"),
                ("flow_mapping_comment", "key: { a: 1, b: 2 }"),
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
                let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
                assert_eq!(rendered.text, expected_text, "{name}");
                let documents = parse_rendered_yaml(&rendered.text).unwrap();
                let value = yaml_mapping_entry(&documents[0], "key").expect("mapping key");
                assert_eq!(yaml_mapping(value).expect("mapping").len(), 2, "{name}");
            }

            let alias_in_flow_mapping_value =
                module.getattr("alias_in_flow_mapping_value").unwrap();
            let alias_in_flow_mapping_value =
                extract_template(py, &alias_in_flow_mapping_value, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&alias_in_flow_mapping_value).unwrap())
                    .unwrap();
            assert_eq!(rendered.text, "base: &a { x: 1 }\nvalue: { ref: *a }");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("value key");
            let referenced = yaml_mapping_entry(value, "ref").expect("ref key");
            assert_integer_entry(referenced, "x", 1);

            let flow_null_and_alias = module.getattr("flow_null_and_alias").unwrap();
            let flow_null_and_alias =
                extract_template(py, &flow_null_and_alias, "yaml_t/yaml_t_str").unwrap();
            let rendered =
                render_document(py, &parse_template(&flow_null_and_alias).unwrap()).unwrap();
            assert_eq!(rendered.text, "base: &a { x: 1 }\nvalue: { null: *a }");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            let value = yaml_mapping_entry(&documents[0], "value").expect("value key");
            assert_eq!(yaml_mapping(value).expect("mapping").len(), 1);
        });
    }

    #[test]
    fn renders_verbatim_root_scalar_text_and_data() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("template=t'--- !<tag:yaml.org,2002:str> hello\\n'\n"),
                pyo3::ffi::c_str!("test_yaml_verbatim_root_scalar.py"),
                pyo3::ffi::c_str!("test_yaml_verbatim_root_scalar"),
            )
            .unwrap();

            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
            assert_eq!(rendered.text, "---\n!<tag:yaml.org,2002:str> hello");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(yaml_scalar_text(&documents[0]), Some("hello"));
        });
    }

    #[test]
    fn renders_verbatim_root_anchor_scalar_text_and_data() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("template=t'--- !<tag:yaml.org,2002:str> &root hello\\n'\n"),
                pyo3::ffi::c_str!("test_yaml_verbatim_root_anchor_scalar.py"),
                pyo3::ffi::c_str!("test_yaml_verbatim_root_anchor_scalar"),
            )
            .unwrap();

            let template = module.getattr("template").unwrap();
            let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
            let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
            assert_eq!(rendered.text, "---\n!<tag:yaml.org,2002:str> &root hello");
            let documents = parse_rendered_yaml(&rendered.text).unwrap();
            assert_eq!(yaml_scalar_text(&documents[0]), Some("hello"));
        });
    }

    #[test]
    fn renders_spec_chapter_2_examples_text_and_data() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "players=t'- Mark McGwire\\n- Sammy Sosa\\n- Ken Griffey\\n'\nclubs=t'american:\\n- Boston Red Sox\\n- Detroit Tigers\\n- New York Yankees\\nnational:\\n- New York Mets\\n- Chicago Cubs\\n- Atlanta Braves\\n'\nstats_seq=t'-\\n  name: Mark McGwire\\n  hr:   65\\n  avg:  0.278\\n-\\n  name: Sammy Sosa\\n  hr:   63\\n  avg:  0.288\\n'\nmap_of_maps=t'Mark McGwire: {{hr: 65, avg: 0.278}}\\nSammy Sosa: {{\\n  hr: 63,\\n  avg: 0.288,\\n}}\\n'\ntwo_docs=t'# Ranking of 1998 home runs\\n---\\n- Mark McGwire\\n- Sammy Sosa\\n- Ken Griffey\\n\\n# Team ranking\\n---\\n- Chicago Cubs\\n- St Louis Cardinals\\n'\nplay_feed=t'---\\ntime: 20:03:20\\nplayer: Sammy Sosa\\naction: strike (miss)\\n...\\n---\\ntime: 20:03:47\\nplayer: Sammy Sosa\\naction: grand slam\\n...\\n'\n"
                ),
                pyo3::ffi::c_str!("test_yaml_spec_chapter_2_examples.py"),
                pyo3::ffi::c_str!("test_yaml_spec_chapter_2_examples"),
            )
            .unwrap();

            for (name, expected_text) in [
                ("players", "- Mark McGwire\n- Sammy Sosa\n- Ken Griffey"),
                (
                    "clubs",
                    "american:\n  - Boston Red Sox\n  - Detroit Tigers\n  - New York Yankees\nnational:\n  - New York Mets\n  - Chicago Cubs\n  - Atlanta Braves",
                ),
                (
                    "stats_seq",
                    "-\n  name: Mark McGwire\n  hr: 65\n  avg: 0.278\n-\n  name: Sammy Sosa\n  hr: 63\n  avg: 0.288",
                ),
                (
                    "map_of_maps",
                    "Mark McGwire: { hr: 65, avg: 0.278 }\nSammy Sosa: { hr: 63, avg: 0.288 }",
                ),
                (
                    "two_docs",
                    "---\n- Mark McGwire\n- Sammy Sosa\n- Ken Griffey\n---\n- Chicago Cubs\n- St Louis Cardinals",
                ),
                (
                    "play_feed",
                    "---\ntime: 20:03:20\nplayer: Sammy Sosa\naction: strike (miss)\n...\n---\ntime: 20:03:47\nplayer: Sammy Sosa\naction: grand slam\n...",
                ),
            ] {
                let template = module.getattr(name).unwrap();
                let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
                let rendered = render_document(py, &parse_template(&template).unwrap()).unwrap();
                assert_eq!(rendered.text, expected_text, "{name}");
                let documents = parse_rendered_yaml(&rendered.text).unwrap();
                match name {
                    "players" => assert_eq!(yaml_sequence_len(&documents[0]), Some(3)),
                    "clubs" => assert_eq!(yaml_mapping(&documents[0]).expect("clubs").len(), 2),
                    "stats_seq" => assert_eq!(yaml_sequence_len(&documents[0]), Some(2)),
                    "map_of_maps" => {
                        assert_eq!(yaml_mapping(&documents[0]).expect("map_of_maps").len(), 2)
                    }
                    "two_docs" => assert_eq!(documents.len(), 2),
                    "play_feed" => assert_eq!(documents.len(), 2),
                    _ => unreachable!(),
                }
            }
        });
    }

    #[test]
    fn test_parse_rendered_yaml_surfaces_parse_failures() {
        let err =
            parse_rendered_yaml("value: *missing\n").expect_err("expected YAML parse failure");
        assert_eq!(err.kind, ErrorKind::Parse);
        assert!(err.message.contains("Rendered YAML could not be reparsed"));
    }
}
