use crate::{BoundTemplate, exact_integer_string};
use pyo3::prelude::*;
use pyo3::types::{PyDate, PyDateTime, PyDict, PyList, PyTime};
use saphyr::{LoadableYamlNode, MappingOwned, ScalarOwned, YamlOwned};
use saphyr_parser::{ScalarStyle, Tag};
use std::borrow::Cow;
use std::collections::HashMap;
use tstring_syntax::{BackendError, BackendResult, TemplateInput, TemplateSegment};
use tstring_yaml::{
    YamlAliasNode, YamlBlockScalarNode, YamlChunk, YamlInterpolationNode, YamlKeyNode,
    YamlKeyValue, YamlMappingNode, YamlPlainScalarNode, YamlProfile, YamlScalarNode,
    YamlStreamNode, YamlValueNode,
};

pub struct YamlRenderOutput {
    pub text: String,
    pub documents: Vec<YamlOwned>,
}

#[derive(Clone, Default)]
enum PreparedFormattedSlot {
    #[default]
    Unresolved,
    Resolved(Option<String>),
}

#[derive(Clone)]
enum StandalonePayloadParseResult {
    Unresolved,
    Missing,
    Invalid(String),
    Valid(YamlValueNode),
}

struct YamlPreparedDocument<'a> {
    template: &'a BoundTemplate,
    formatted_slots: Vec<PreparedFormattedSlot>,
    standalone_payloads: Vec<StandalonePayloadParseResult>,
}

impl<'a> YamlPreparedDocument<'a> {
    fn new(template: &'a BoundTemplate) -> Self {
        Self {
            template,
            formatted_slots: vec![
                PreparedFormattedSlot::Unresolved;
                template.interpolation_count()
            ],
            standalone_payloads: vec![
                StandalonePayloadParseResult::Unresolved;
                template.interpolation_count()
            ],
        }
    }

    fn formatted_text(
        &mut self,
        py: Python<'_>,
        interpolation_index: usize,
        span: tstring_syntax::SourceSpan,
    ) -> BackendResult<Option<String>> {
        let Some(slot) = self.formatted_slots.get_mut(interpolation_index) else {
            return Err(BackendError::semantic(format!(
                "Missing prepared YAML slot for interpolation index {interpolation_index}."
            )));
        };
        match slot {
            PreparedFormattedSlot::Resolved(formatted) => Ok(formatted.clone()),
            PreparedFormattedSlot::Unresolved => {
                let formatted = self.template.formatted_interpolation_text(
                    py,
                    interpolation_index,
                    Some(span),
                )?;
                *slot = PreparedFormattedSlot::Resolved(formatted.clone());
                Ok(formatted)
            }
        }
    }

    fn standalone_payload(
        &mut self,
        py: Python<'_>,
        profile: YamlProfile,
        interpolation_index: usize,
        expression: &str,
        span: &tstring_syntax::SourceSpan,
    ) -> BackendResult<Option<YamlValueNode>> {
        let formatted = self.formatted_text(py, interpolation_index, span.clone())?;
        let Some(slot) = self.standalone_payloads.get_mut(interpolation_index) else {
            return Err(BackendError::semantic(format!(
                "Missing prepared YAML payload slot for interpolation index {interpolation_index}."
            )));
        };
        match slot {
            StandalonePayloadParseResult::Missing => Ok(None),
            StandalonePayloadParseResult::Valid(value) => Ok(Some(value.clone())),
            StandalonePayloadParseResult::Invalid(message) => Err(BackendError::parse_at(
                "yaml.parse",
                message.clone(),
                Some(span.clone()),
            )),
            StandalonePayloadParseResult::Unresolved => {
                let Some(formatted) = formatted else {
                    *slot = StandalonePayloadParseResult::Missing;
                    return Ok(None);
                };
                let fragment =
                    TemplateInput::from_segments(vec![TemplateSegment::StaticText(formatted)]);
                let stream = tstring_yaml::parse_template_with_profile(&fragment, profile)
                    .map_err(|err| {
                        BackendError::parse_at(
                            "yaml.parse",
                            format!(
                                "Interpolation {:?} produced invalid formatted YAML payload: {err}",
                                expression
                            ),
                            Some(span.clone()),
                        )
                    })?;
                if stream.documents.len() != 1 {
                    let message = formatted_yaml_document_structure_message(expression);
                    *slot = StandalonePayloadParseResult::Invalid(message.clone());
                    return Err(BackendError::parse_at(
                        "yaml.parse",
                        message,
                        Some(span.clone()),
                    ));
                }
                let document = &stream.documents[0];
                if !document.directives.is_empty()
                    || document.explicit_start
                    || document.explicit_end
                {
                    let message = formatted_yaml_document_structure_message(expression);
                    *slot = StandalonePayloadParseResult::Invalid(message.clone());
                    return Err(BackendError::parse_at(
                        "yaml.parse",
                        message,
                        Some(span.clone()),
                    ));
                }
                *slot = StandalonePayloadParseResult::Valid(document.value.clone());
                Ok(Some(document.value.clone()))
            }
        }
    }
}

struct DocumentContextPlan {
    directives: HashMap<String, String>,
    validated_payload_sites: Vec<usize>,
}

impl DocumentContextPlan {
    fn new(directives: HashMap<String, String>) -> Self {
        Self {
            directives,
            validated_payload_sites: Vec::new(),
        }
    }
}

#[derive(Default)]
struct ValidationContext {
    anchors: HashMap<String, ()>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ValidationMode {
    Text,
    Data,
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

pub fn render_document_text(
    py: Python<'_>,
    template: &BoundTemplate,
    profile: YamlProfile,
    node: &YamlStreamNode,
) -> BackendResult<String> {
    let mut prepared = YamlPreparedDocument::new(template);
    let (text, _) = render_and_validate_stream(py, &mut prepared, profile, node)?;
    Ok(text)
}

pub fn render_document_data(
    py: Python<'_>,
    template: &BoundTemplate,
    profile: YamlProfile,
    node: &YamlStreamNode,
) -> BackendResult<Vec<YamlOwned>> {
    let mut prepared = YamlPreparedDocument::new(template);
    let plans = validate_stream(py, &mut prepared, profile, node, ValidationMode::Data)?;
    let mut documents = Vec::with_capacity(node.documents.len());
    for (document, plan) in node.documents.iter().zip(plans.iter()) {
        let materialized_value =
            materialize_document_value(py, &mut prepared, profile, &document.value, plan)?;
        documents.push(materialized_value);
    }
    Ok(documents)
}

pub fn render_document(
    py: Python<'_>,
    template: &BoundTemplate,
    profile: YamlProfile,
    node: &YamlStreamNode,
) -> BackendResult<YamlRenderOutput> {
    let mut prepared = YamlPreparedDocument::new(template);
    let (text, plans) = render_and_validate_stream(py, &mut prepared, profile, node)?;
    let mut documents = Vec::with_capacity(node.documents.len());
    for (document, plan) in node.documents.iter().zip(plans.iter()) {
        let materialized_value =
            materialize_document_value(py, &mut prepared, profile, &document.value, plan)?;
        documents.push(materialized_value);
    }
    Ok(YamlRenderOutput { text, documents })
}

fn validate_stream(
    py: Python<'_>,
    prepared: &mut YamlPreparedDocument<'_>,
    profile: YamlProfile,
    node: &YamlStreamNode,
    mode: ValidationMode,
) -> BackendResult<Vec<DocumentContextPlan>> {
    let mut plans = Vec::with_capacity(node.documents.len());
    for document in &node.documents {
        let directives = parse_tag_directives(&document.directives);
        let mut plan = DocumentContextPlan::new(directives);
        let mut validation = ValidationContext::default();
        validate_value(
            py,
            prepared,
            profile,
            &document.value,
            &mut validation,
            &mut plan,
            CollectionRenderContext::BlockAllowed,
            mode,
        )?;
        plans.push(plan);
    }
    Ok(plans)
}

fn render_and_validate_stream(
    py: Python<'_>,
    prepared: &mut YamlPreparedDocument<'_>,
    profile: YamlProfile,
    node: &YamlStreamNode,
) -> BackendResult<(String, Vec<DocumentContextPlan>)> {
    let mut parts = Vec::new();
    let mut plans = Vec::with_capacity(node.documents.len());
    for document in &node.documents {
        let directives = parse_tag_directives(&document.directives);
        let mut plan = DocumentContextPlan::new(directives.clone());
        let mut validation = ValidationContext::default();
        let mut lines = Vec::new();
        lines.extend(document.directives.iter().cloned());
        if document.explicit_start || !document.directives.is_empty() || node.documents.len() > 1 {
            lines.push("---".to_owned());
        }
        lines.push(
            render_value(
                py,
                prepared,
                profile,
                &document.value,
                0,
                &mut validation,
                &mut plan,
                CollectionRenderContext::BlockAllowed,
                ValidationMode::Text,
            )?
            .text,
        );
        if document.explicit_end {
            lines.push("...".to_owned());
        }
        parts.push(lines.join("\n"));
        plans.push(plan);
    }
    Ok((parts.join("\n"), plans))
}

struct MaterializeContext {
    anchors: HashMap<String, YamlOwned>,
}

impl MaterializeContext {
    fn new() -> Self {
        Self {
            anchors: HashMap::new(),
        }
    }

    fn insert_anchor(&mut self, name: String, value: YamlOwned) -> BackendResult<()> {
        self.anchors.insert(name, value);
        Ok(())
    }

    fn resolve_alias(&self, name: &str) -> BackendResult<YamlOwned> {
        self.anchors.get(name).cloned().ok_or_else(|| {
            BackendError::semantic(format!(
                "Rendered YAML contains unknown anchor {name:?} during data materialization."
            ))
        })
    }
}

fn materialize_document_value(
    py: Python<'_>,
    prepared: &mut YamlPreparedDocument<'_>,
    profile: YamlProfile,
    node: &YamlValueNode,
    plan: &DocumentContextPlan,
) -> BackendResult<YamlOwned> {
    let mut context = MaterializeContext::new();
    materialize_value(py, prepared, profile, node, &mut context, &plan.directives)
}

fn materialize_value(
    py: Python<'_>,
    prepared: &mut YamlPreparedDocument<'_>,
    profile: YamlProfile,
    node: &YamlValueNode,
    context: &mut MaterializeContext,
    directives: &HashMap<String, String>,
) -> BackendResult<YamlOwned> {
    match node {
        YamlValueNode::Decorated(node) => {
            materialize_decorated_value(py, prepared, profile, node, context, directives)
        }
        YamlValueNode::Mapping(node) => {
            materialize_mapping(py, prepared, profile, node, context, directives)
        }
        YamlValueNode::Sequence(node) => {
            materialize_sequence(py, prepared, profile, node, context, directives)
        }
        YamlValueNode::Interpolation(node) => {
            materialize_interpolation_value(py, prepared, profile, node, false, context, directives)
        }
        YamlValueNode::Scalar(YamlScalarNode::Alias(node)) => {
            let name = assemble_chunks(py, prepared, &node.chunks, true)?;
            context.resolve_alias(&name)
        }
        YamlValueNode::Scalar(YamlScalarNode::Block(node)) => {
            materialize_block_scalar(py, prepared, node, 0)
        }
        YamlValueNode::Scalar(YamlScalarNode::DoubleQuoted(node)) => {
            materialize_scalar(py, prepared, &node.chunks, ScalarStyle::DoubleQuoted, None)
        }
        YamlValueNode::Scalar(YamlScalarNode::SingleQuoted(node)) => {
            materialize_scalar(py, prepared, &node.chunks, ScalarStyle::SingleQuoted, None)
        }
        YamlValueNode::Scalar(YamlScalarNode::Plain(node)) => {
            materialize_scalar(py, prepared, &node.chunks, ScalarStyle::Plain, None)
        }
    }
}

fn materialize_decorated_value(
    py: Python<'_>,
    prepared: &mut YamlPreparedDocument<'_>,
    profile: YamlProfile,
    node: &tstring_yaml::YamlDecoratedNode,
    context: &mut MaterializeContext,
    directives: &HashMap<String, String>,
) -> BackendResult<YamlOwned> {
    let tag = node
        .tag
        .as_ref()
        .map(|tag| materialize_tag(py, prepared, tag, directives))
        .transpose()?;
    let value = match node.value.as_ref() {
        YamlValueNode::Scalar(YamlScalarNode::Block(block)) => {
            let value = materialize_block_scalar(py, prepared, block, 0)?;
            if let Some(tag) = tag.clone() {
                YamlOwned::Tagged(tag, Box::new(value))
            } else {
                value
            }
        }
        YamlValueNode::Scalar(YamlScalarNode::DoubleQuoted(scalar)) => materialize_scalar(
            py,
            prepared,
            &scalar.chunks,
            ScalarStyle::DoubleQuoted,
            tag.as_ref(),
        )?,
        YamlValueNode::Scalar(YamlScalarNode::SingleQuoted(scalar)) => materialize_scalar(
            py,
            prepared,
            &scalar.chunks,
            ScalarStyle::SingleQuoted,
            tag.as_ref(),
        )?,
        YamlValueNode::Scalar(YamlScalarNode::Plain(scalar)) => materialize_scalar(
            py,
            prepared,
            &scalar.chunks,
            ScalarStyle::Plain,
            tag.as_ref(),
        )?,
        inner => {
            let value = materialize_value(py, prepared, profile, inner, context, directives)?;
            if let Some(tag) = tag.clone() {
                YamlOwned::Tagged(tag, Box::new(value))
            } else {
                value
            }
        }
    };

    if let Some(anchor) = &node.anchor {
        let name = assemble_chunks(py, prepared, &anchor.chunks, true)?;
        context.insert_anchor(name, value.clone())?;
    }
    Ok(value)
}

fn materialize_scalar(
    py: Python<'_>,
    prepared: &mut YamlPreparedDocument<'_>,
    chunks: &[YamlChunk],
    style: ScalarStyle,
    tag: Option<&Tag>,
) -> BackendResult<YamlOwned> {
    let mut text = assemble_chunks(py, prepared, chunks, false)?;
    match style {
        ScalarStyle::Plain => {
            text = normalize_plain_scalar_text(&text);
        }
        ScalarStyle::SingleQuoted => {
            text = normalize_single_quoted_scalar_text(&text);
        }
        _ => {}
    }
    Ok(materialize_scalar_value(text, style, tag))
}

fn materialize_block_scalar(
    py: Python<'_>,
    prepared: &mut YamlPreparedDocument<'_>,
    node: &YamlBlockScalarNode,
    indent: usize,
) -> BackendResult<YamlOwned> {
    let rendered = render_block_scalar(py, prepared, node, indent.max(2))?;
    let mut documents = YamlOwned::load_from_str(&rendered).map_err(|err| {
        BackendError::semantic(format!(
            "Rendered YAML block scalar could not be materialized for data output: {err}"
        ))
    })?;
    Ok(documents.pop().unwrap_or(YamlOwned::BadValue))
}

fn materialize_mapping(
    py: Python<'_>,
    prepared: &mut YamlPreparedDocument<'_>,
    profile: YamlProfile,
    node: &YamlMappingNode,
    context: &mut MaterializeContext,
    directives: &HashMap<String, String>,
) -> BackendResult<YamlOwned> {
    let mut mapping = MappingOwned::new();
    for entry in &node.entries {
        let key = materialize_key(py, prepared, profile, &entry.key, context, directives)?;
        let value = materialize_value(py, prepared, profile, &entry.value, context, directives)?;
        mapping.insert(key, value);
    }
    Ok(YamlOwned::Mapping(mapping))
}

fn materialize_sequence(
    py: Python<'_>,
    prepared: &mut YamlPreparedDocument<'_>,
    profile: YamlProfile,
    node: &tstring_yaml::YamlSequenceNode,
    context: &mut MaterializeContext,
    directives: &HashMap<String, String>,
) -> BackendResult<YamlOwned> {
    node.items
        .iter()
        .map(|item| materialize_value(py, prepared, profile, item, context, directives))
        .collect::<BackendResult<Vec<_>>>()
        .map(YamlOwned::Sequence)
}

fn materialize_key(
    py: Python<'_>,
    prepared: &mut YamlPreparedDocument<'_>,
    profile: YamlProfile,
    node: &YamlKeyNode,
    context: &mut MaterializeContext,
    directives: &HashMap<String, String>,
) -> BackendResult<YamlOwned> {
    match &node.value {
        YamlKeyValue::Interpolation(value) => {
            materialize_interpolation_value(py, prepared, profile, value, true, context, directives)
        }
        YamlKeyValue::Scalar(YamlScalarNode::Alias(node)) => {
            let name = assemble_chunks(py, prepared, &node.chunks, true)?;
            context.resolve_alias(&name)
        }
        YamlKeyValue::Scalar(YamlScalarNode::Block(node)) => {
            materialize_block_scalar(py, prepared, node, 0)
        }
        YamlKeyValue::Scalar(YamlScalarNode::DoubleQuoted(node)) => {
            materialize_scalar(py, prepared, &node.chunks, ScalarStyle::DoubleQuoted, None)
        }
        YamlKeyValue::Scalar(YamlScalarNode::SingleQuoted(node)) => {
            materialize_scalar(py, prepared, &node.chunks, ScalarStyle::SingleQuoted, None)
        }
        YamlKeyValue::Scalar(YamlScalarNode::Plain(node)) => {
            materialize_scalar(py, prepared, &node.chunks, ScalarStyle::Plain, None)
        }
        YamlKeyValue::Complex(value) => {
            materialize_value(py, prepared, profile, value, context, directives)
        }
    }
}

fn materialize_interpolation_value(
    py: Python<'_>,
    prepared: &mut YamlPreparedDocument<'_>,
    profile: YamlProfile,
    node: &YamlInterpolationNode,
    key_mode: bool,
    context: &mut MaterializeContext,
    directives: &HashMap<String, String>,
) -> BackendResult<YamlOwned> {
    if let Some(payload) = prepared.standalone_payload(
        py,
        profile,
        node.interpolation_index,
        &prepared.template.expression_label(node.interpolation_index),
        &node.span,
    )? {
        return materialize_value(py, prepared, profile, &payload, context, directives);
    }
    let bound = prepared.template.bind_value(py, node.interpolation_index)?;
    materialize_python_value(&bound, key_mode, Some(node.span.clone()))
}

fn materialize_python_value(
    value: &Bound<'_, PyAny>,
    key_mode: bool,
    span: Option<tstring_syntax::SourceSpan>,
) -> BackendResult<YamlOwned> {
    if value.is_none() {
        return Ok(YamlOwned::Value(ScalarOwned::Null));
    }
    if let Ok(value) = value.extract::<bool>() {
        return Ok(YamlOwned::Value(ScalarOwned::Boolean(value)));
    }
    if let Some(value_text) = exact_integer_string(value).map_err(|err| {
        BackendError::unrepresentable_at(
            "yaml.unrepresentable.integer",
            format!("YAML interpolation could not be rendered as an exact integer: {err}"),
            span.clone(),
        )
    })? {
        return Ok(YamlOwned::Value(ScalarOwned::parse_from_cow(Cow::Owned(
            value_text,
        ))));
    }
    if let Ok(value) = value.extract::<f64>() {
        if !value.is_finite() {
            return Err(BackendError::unrepresentable_at(
                "yaml.unrepresentable.float",
                "YAML interpolation resolved to a non-finite float, which this backend does not represent.",
                span,
            ));
        }
        return Ok(YamlOwned::Value(ScalarOwned::parse_from_cow(Cow::Owned(
            value.to_string(),
        ))));
    }
    if let Ok(value) = value.extract::<String>() {
        return Ok(YamlOwned::Value(ScalarOwned::String(value)));
    }
    if let Ok(value) = value.downcast::<PyDateTime>() {
        let rendered = value
            .call_method0("isoformat")
            .and_then(|value| value.extract::<String>())
            .map_err(|err| {
                BackendError::unrepresentable_at(
                    "yaml.unrepresentable.datetime",
                    err.to_string(),
                    span,
                )
            })?;
        let scalar = ScalarOwned::parse_from_cow_and_metadata(
            Cow::Owned(rendered),
            ScalarStyle::Plain,
            None,
        )
        .unwrap_or_else(|| ScalarOwned::String(String::new()));
        return Ok(YamlOwned::Value(scalar));
    }
    if let Ok(value) = value.downcast::<PyDate>() {
        let rendered = value
            .call_method0("isoformat")
            .and_then(|value| value.extract::<String>())
            .map_err(|err| {
                BackendError::unrepresentable_at("yaml.unrepresentable.date", err.to_string(), span)
            })?;
        let scalar = ScalarOwned::parse_from_cow_and_metadata(
            Cow::Owned(rendered),
            ScalarStyle::Plain,
            None,
        )
        .unwrap_or_else(|| ScalarOwned::String(String::new()));
        return Ok(YamlOwned::Value(scalar));
    }
    if let Ok(value) = value.downcast::<PyTime>() {
        let rendered = value
            .call_method0("isoformat")
            .and_then(|value| value.extract::<String>())
            .map_err(|err| {
                BackendError::unrepresentable_at("yaml.unrepresentable.time", err.to_string(), span)
            })?;
        let scalar = ScalarOwned::parse_from_cow_and_metadata(
            Cow::Owned(rendered),
            ScalarStyle::Plain,
            None,
        )
        .unwrap_or_else(|| ScalarOwned::String(String::new()));
        return Ok(YamlOwned::Value(scalar));
    }
    if let Ok(list) = value.downcast::<PyList>() {
        let items = list
            .iter()
            .map(|item| materialize_python_value(&item, false, span.clone()))
            .collect::<BackendResult<Vec<_>>>()?;
        return Ok(YamlOwned::Sequence(items));
    }
    if let Ok(dict) = value.downcast::<PyDict>() {
        let mut mapping = MappingOwned::new();
        for (key, item) in dict.iter() {
            let key_value = if key.is_none()
                || key.extract::<String>().is_ok()
                || exact_integer_string(&key)
                    .map(|value| value.is_some())
                    .unwrap_or(false)
                || key.extract::<f64>().is_ok()
                || key.extract::<bool>().is_ok()
            {
                materialize_python_value(&key, true, span.clone())?
            } else {
                return Err(BackendError::unrepresentable_at(
                    "yaml.unrepresentable.key",
                    format!(
                        "YAML key interpolation resolved to unsupported key type {}.",
                        type_name(&key)
                    ),
                    span,
                ));
            };
            mapping.insert(
                key_value,
                materialize_python_value(&item, false, span.clone())?,
            );
        }
        return Ok(YamlOwned::Mapping(mapping));
    }
    if key_mode {
        let text = value
            .str()
            .map_err(|err| {
                BackendError::unrepresentable_at(
                    "yaml.unrepresentable.key",
                    err.to_string(),
                    span.clone(),
                )
            })?
            .extract::<String>()
            .map_err(|err| {
                BackendError::unrepresentable_at(
                    "yaml.unrepresentable.key",
                    err.to_string(),
                    span.clone(),
                )
            })?;
        return Ok(YamlOwned::Value(ScalarOwned::String(text)));
    }
    Err(BackendError::unrepresentable_at(
        "yaml.unrepresentable.value",
        format!(
            "Interpolation could not be rendered as YAML from {}.",
            type_name(value)
        ),
        span,
    ))
}

fn materialize_scalar_value(text: String, style: ScalarStyle, tag: Option<&Tag>) -> YamlOwned {
    if style == ScalarStyle::Plain && text.is_empty() && tag.is_none() {
        return YamlOwned::Value(ScalarOwned::Null);
    }
    match tag {
        Some(tag) if !tag.is_yaml_core_schema() => YamlOwned::Tagged(
            tag.clone(),
            Box::new(YamlOwned::Representation(text, style, None)),
        ),
        _ => YamlOwned::Value(
            ScalarOwned::parse_from_cow_and_metadata(
                Cow::Owned(text),
                style,
                tag.map(Cow::Borrowed).as_ref(),
            )
            .unwrap_or_else(|| ScalarOwned::String(String::new())),
        ),
    }
}

fn materialize_tag(
    py: Python<'_>,
    prepared: &mut YamlPreparedDocument<'_>,
    tag: &tstring_yaml::YamlTagNode,
    directives: &HashMap<String, String>,
) -> BackendResult<Tag> {
    let suffix = assemble_chunks(py, prepared, &tag.chunks, true)?;
    let mut tag = parse_materialized_tag(&format!("!{suffix}"))?;
    if let Some(prefix) = directives.get(&tag.handle) {
        tag.handle = prefix.clone();
    } else if !matches!(tag.handle.as_str(), "!" | "!!" | "tag:yaml.org,2002:") {
        return Err(BackendError::semantic(format!(
            "Rendered YAML uses undeclared tag handle {:?}.",
            tag.handle
        )));
    }
    Ok(tag)
}

fn parse_tag_directives(directives: &[String]) -> HashMap<String, String> {
    let mut handles = HashMap::new();
    for directive in directives {
        let mut parts = directive.split_whitespace();
        if parts.next() == Some("%TAG")
            && let (Some(handle), Some(prefix)) = (parts.next(), parts.next())
        {
            handles.insert(handle.to_owned(), prefix.to_owned());
        }
    }
    handles
}

fn parse_materialized_tag(tag: &str) -> BackendResult<Tag> {
    if let Some(suffix) = tag.strip_prefix("tag:yaml.org,2002:") {
        return Ok(Tag {
            handle: "tag:yaml.org,2002:".to_owned(),
            suffix: suffix.to_owned(),
        });
    }
    if let Some(inner) = tag
        .strip_prefix("!<")
        .and_then(|value| value.strip_suffix('>'))
    {
        if let Some(suffix) = inner.strip_prefix("tag:yaml.org,2002:") {
            return Ok(Tag {
                handle: "tag:yaml.org,2002:".to_owned(),
                suffix: suffix.to_owned(),
            });
        }
        return Ok(Tag {
            handle: "!".to_owned(),
            suffix: inner.to_owned(),
        });
    }
    if let Some(suffix) = tag.strip_prefix("!!") {
        return Ok(Tag {
            handle: "!!".to_owned(),
            suffix: suffix.to_owned(),
        });
    }
    if let Some(rest) = tag.strip_prefix('!') {
        if let Some(separator) = rest.find('!') {
            return Ok(Tag {
                handle: format!("!{}!", &rest[..separator]),
                suffix: rest[separator + 1..].to_owned(),
            });
        }
        return Ok(Tag {
            handle: "!".to_owned(),
            suffix: rest.to_owned(),
        });
    }
    Ok(Tag {
        handle: "!".to_owned(),
        suffix: tag.to_owned(),
    })
}

fn formatted_yaml_document_structure_message(expression: &str) -> String {
    format!(
        "Interpolation {:?} produced unsupported YAML document-level structure.",
        expression
    )
}

fn normalize_plain_scalar_text(text: &str) -> String {
    text.lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalize_single_quoted_scalar_text(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    if !normalized.contains('\n') {
        return normalized;
    }
    let lines = normalized.split('\n').collect::<Vec<_>>();
    let mut folded = lines.first().copied().unwrap_or_default().to_owned();
    let mut index = 1usize;

    while index < lines.len() {
        let mut breaks = 1usize;
        while index < lines.len() - 1 && lines[index].trim_matches([' ', '\t']).is_empty() {
            breaks += 1;
            index += 1;
        }

        let next_line = lines[index].trim_matches([' ', '\t']);
        trim_yaml_flow_folding_whitespace(&mut folded);
        if breaks == 1 {
            folded.push(' ');
        } else {
            folded.push_str(&"\n".repeat(breaks - 1));
        }
        folded.push_str(next_line);
        index += 1;
    }

    folded
}

fn trim_yaml_flow_folding_whitespace(text: &mut String) {
    let trimmed_len = text.trim_end_matches([' ', '\t']).len();
    text.truncate(trimmed_len);
}

struct PreparedDecoratedValue<'a> {
    inner: &'a YamlValueNode,
    anchor_name: Option<String>,
    prefix: String,
}

fn prepare_decorated_value<'a>(
    py: Python<'_>,
    prepared: &mut YamlPreparedDocument<'_>,
    node: &'a YamlValueNode,
    directives: &HashMap<String, String>,
    include_prefix: bool,
) -> BackendResult<PreparedDecoratedValue<'a>> {
    let mut prefix = String::new();
    let mut anchor_name = None;

    let inner = if let YamlValueNode::Decorated(node) = node {
        if let Some(tag) = &node.tag {
            let _ = materialize_tag(py, prepared, tag, directives)?;
            let rendered_tag = assemble_chunks(py, prepared, &tag.chunks, true)?;
            if include_prefix {
                prefix.push('!');
                prefix.push_str(&rendered_tag);
            }
        }
        if let Some(anchor) = &node.anchor {
            let rendered_anchor = assemble_chunks(py, prepared, &anchor.chunks, true)?;
            if include_prefix {
                if !prefix.is_empty() {
                    prefix.push(' ');
                }
                prefix.push('&');
                prefix.push_str(&rendered_anchor);
            }
            anchor_name = Some(rendered_anchor);
        }
        node.value.as_ref()
    } else {
        node
    };

    Ok(PreparedDecoratedValue {
        inner,
        anchor_name,
        prefix,
    })
}

fn register_anchor(validation: &mut ValidationContext, anchor_name: Option<String>) {
    if let Some(anchor_name) = anchor_name {
        validation.anchors.insert(anchor_name, ());
    }
}

fn validate_alias_name(
    py: Python<'_>,
    prepared: &mut YamlPreparedDocument<'_>,
    validation: &ValidationContext,
    node: &YamlAliasNode,
) -> BackendResult<String> {
    let name = assemble_chunks(py, prepared, &node.chunks, true)?;
    if !validation.anchors.contains_key(&name) {
        return Err(BackendError::semantic(format!(
            "Rendered YAML contains unknown anchor {name:?} during text validation."
        )));
    }
    Ok(name)
}

fn validate_value_interpolation(
    py: Python<'_>,
    prepared: &mut YamlPreparedDocument<'_>,
    profile: YamlProfile,
    validation: &mut ValidationContext,
    plan: &mut DocumentContextPlan,
    node: &YamlInterpolationNode,
    indent: usize,
    context: CollectionRenderContext,
    mode: ValidationMode,
) -> BackendResult<RenderedYamlValue> {
    let expression = prepared.template.expression_label(node.interpolation_index);
    if let Some(formatted) =
        prepared.formatted_text(py, node.interpolation_index, node.span.clone())?
    {
        if let Some(payload) = prepared.standalone_payload(
            py,
            profile,
            node.interpolation_index,
            &expression,
            &node.span,
        )? {
            plan.validated_payload_sites.push(node.interpolation_index);
            let rendered_payload = payload_layout(&payload);
            if mode == ValidationMode::Text {
                ensure_layout_allowed(
                    &expression,
                    &node.span,
                    CollectionRenderContext::FlowRequired,
                    rendered_payload.layout,
                )?;
            }
            validate_value(
                py, prepared, profile, &payload, validation, plan, context, mode,
            )?;
            return Ok(RenderedYamlValue {
                text: formatted,
                layout: rendered_payload.layout,
                empty_collection: rendered_payload.empty_collection,
            });
        }
        return Ok(RenderedYamlValue::inline(formatted));
    }

    let value = prepared.template.bind_value(py, node.interpolation_index)?;
    if value.downcast::<PyList>().is_ok() || value.downcast::<PyDict>().is_ok() {
        let owned = materialize_python_value(&value, false, Some(node.span.clone()))?;
        return render_owned_value(&owned, indent, context);
    }
    render_python_value(&value, false, Some(node.span.clone())).map(RenderedYamlValue::inline)
}

fn payload_layout(node: &YamlValueNode) -> RenderedYamlValue {
    match node {
        YamlValueNode::Mapping(node) => {
            if node.flow || node.entries.is_empty() {
                RenderedYamlValue::flow(String::new(), node.entries.is_empty())
            } else {
                RenderedYamlValue::block(String::new())
            }
        }
        YamlValueNode::Sequence(node) => {
            if node.flow || node.items.is_empty() {
                RenderedYamlValue::flow(String::new(), node.items.is_empty())
            } else {
                RenderedYamlValue::block(String::new())
            }
        }
        YamlValueNode::Decorated(node) => payload_layout(node.value.as_ref()),
        _ => RenderedYamlValue::inline(String::new()),
    }
}

fn ensure_layout_allowed(
    expression: &str,
    span: &tstring_syntax::SourceSpan,
    context: CollectionRenderContext,
    layout: RenderedLayout,
) -> BackendResult<()> {
    if context == CollectionRenderContext::FlowRequired && layout == RenderedLayout::Block {
        return Err(BackendError::parse_at(
            "yaml.parse",
            format!(
                "Interpolation {:?} produced a block YAML payload where flow-safe formatted text is required.",
                expression
            ),
            Some(span.clone()),
        ));
    }
    Ok(())
}

fn validate_value(
    py: Python<'_>,
    prepared: &mut YamlPreparedDocument<'_>,
    profile: YamlProfile,
    node: &YamlValueNode,
    validation: &mut ValidationContext,
    plan: &mut DocumentContextPlan,
    context: CollectionRenderContext,
    mode: ValidationMode,
) -> BackendResult<()> {
    let prepared_value = prepare_decorated_value(py, prepared, node, &plan.directives, false)?;
    let inner = prepared_value.inner;

    match inner {
        YamlValueNode::Mapping(node) => {
            let child_context = if node.flow {
                CollectionRenderContext::FlowRequired
            } else {
                CollectionRenderContext::BlockAllowed
            };
            for entry in &node.entries {
                validate_key(py, prepared, profile, &entry.key, validation, plan, mode)?;
                validate_value(
                    py,
                    prepared,
                    profile,
                    &entry.value,
                    validation,
                    plan,
                    child_context,
                    mode,
                )?;
            }
        }
        YamlValueNode::Sequence(node) => {
            let child_context = if node.flow {
                CollectionRenderContext::FlowRequired
            } else {
                CollectionRenderContext::BlockAllowed
            };
            for item in &node.items {
                validate_value(
                    py,
                    prepared,
                    profile,
                    item,
                    validation,
                    plan,
                    child_context,
                    mode,
                )?;
            }
        }
        YamlValueNode::Interpolation(node) => {
            let _ = validate_value_interpolation(
                py, prepared, profile, validation, plan, node, 0, context, mode,
            )?;
        }
        YamlValueNode::Scalar(YamlScalarNode::Alias(node)) => {
            let _ = validate_alias_name(py, prepared, validation, node)?;
        }
        YamlValueNode::Scalar(YamlScalarNode::Block(node)) => {
            let _ = assemble_chunks(py, prepared, &node.chunks, false)?;
        }
        YamlValueNode::Scalar(YamlScalarNode::DoubleQuoted(node)) => {
            let _ = assemble_chunks(py, prepared, &node.chunks, false)?;
        }
        YamlValueNode::Scalar(YamlScalarNode::SingleQuoted(node)) => {
            let _ = assemble_chunks(py, prepared, &node.chunks, false)?;
        }
        YamlValueNode::Scalar(YamlScalarNode::Plain(node)) => {
            let _ = assemble_chunks(py, prepared, &node.chunks, false)?;
        }
        YamlValueNode::Decorated(_) => {
            unreachable!("decorated values are unwrapped before validation")
        }
    }

    register_anchor(validation, prepared_value.anchor_name);

    Ok(())
}

fn validate_key(
    py: Python<'_>,
    prepared: &mut YamlPreparedDocument<'_>,
    profile: YamlProfile,
    node: &YamlKeyNode,
    validation: &mut ValidationContext,
    plan: &mut DocumentContextPlan,
    mode: ValidationMode,
) -> BackendResult<()> {
    match &node.value {
        YamlKeyValue::Interpolation(value) => {
            if prepared
                .formatted_text(py, value.interpolation_index, value.span.clone())?
                .is_none()
            {
                let _ = render_python_value(
                    prepared
                        .template
                        .bind_value(py, value.interpolation_index)?,
                    true,
                    Some(value.span.clone()),
                )?;
            }
        }
        YamlKeyValue::Scalar(YamlScalarNode::Alias(node)) => {
            let name = assemble_chunks(py, prepared, &node.chunks, true)?;
            if !validation.anchors.contains_key(&name) {
                return Err(BackendError::semantic(format!(
                    "Rendered YAML contains unknown anchor {name:?} during text validation."
                )));
            }
        }
        YamlKeyValue::Scalar(YamlScalarNode::Block(node)) => {
            let _ = assemble_chunks(py, prepared, &node.chunks, false)?;
        }
        YamlKeyValue::Scalar(YamlScalarNode::DoubleQuoted(node)) => {
            let _ = assemble_chunks(py, prepared, &node.chunks, false)?;
        }
        YamlKeyValue::Scalar(YamlScalarNode::SingleQuoted(node)) => {
            let _ = assemble_chunks(py, prepared, &node.chunks, false)?;
        }
        YamlKeyValue::Scalar(YamlScalarNode::Plain(node)) => {
            let _ = assemble_chunks(py, prepared, &node.chunks, false)?;
        }
        YamlKeyValue::Complex(value) => {
            validate_value(
                py,
                prepared,
                profile,
                value,
                validation,
                plan,
                CollectionRenderContext::FlowRequired,
                mode,
            )?;
        }
    }
    Ok(())
}

fn render_value(
    py: Python<'_>,
    prepared: &mut YamlPreparedDocument<'_>,
    profile: YamlProfile,
    node: &YamlValueNode,
    indent: usize,
    validation: &mut ValidationContext,
    plan: &mut DocumentContextPlan,
    context: CollectionRenderContext,
    mode: ValidationMode,
) -> BackendResult<RenderedYamlValue> {
    let prepared_value = prepare_decorated_value(py, prepared, node, &plan.directives, true)?;
    let inner = prepared_value.inner;

    let rendered = match inner {
        YamlValueNode::Mapping(node) => {
            render_mapping(py, prepared, profile, node, indent, validation, plan)?
        }
        YamlValueNode::Sequence(node) => {
            render_sequence(py, prepared, profile, node, indent, validation, plan)?
        }
        YamlValueNode::Interpolation(node) => validate_value_interpolation(
            py, prepared, profile, validation, plan, node, indent, context, mode,
        )?,
        YamlValueNode::Scalar(YamlScalarNode::Alias(node)) => {
            let name = validate_alias_name(py, prepared, validation, node)?;
            RenderedYamlValue::inline(format!("*{name}"))
        }
        YamlValueNode::Scalar(YamlScalarNode::Block(node)) => {
            RenderedYamlValue::inline(render_block_scalar(py, prepared, node, indent)?)
        }
        YamlValueNode::Scalar(YamlScalarNode::DoubleQuoted(node)) => RenderedYamlValue::inline(
            serde_json::to_string(&assemble_chunks(py, prepared, &node.chunks, false)?).unwrap(),
        ),
        YamlValueNode::Scalar(YamlScalarNode::SingleQuoted(node)) => {
            RenderedYamlValue::inline(format!(
                "'{}'",
                assemble_chunks(py, prepared, &node.chunks, false)?.replace('\'', "''")
            ))
        }
        YamlValueNode::Scalar(YamlScalarNode::Plain(node)) => {
            RenderedYamlValue::inline(render_plain_scalar(py, prepared, node)?)
        }
        YamlValueNode::Decorated(_) => {
            unreachable!("decorated values are unwrapped before rendering")
        }
    };
    register_anchor(validation, prepared_value.anchor_name);
    if prepared_value.prefix.is_empty() {
        return Ok(rendered);
    }
    Ok(apply_rendered_prefix(prepared_value.prefix, rendered))
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

fn render_mapping(
    py: Python<'_>,
    prepared: &mut YamlPreparedDocument<'_>,
    profile: YamlProfile,
    node: &YamlMappingNode,
    indent: usize,
    validation: &mut ValidationContext,
    plan: &mut DocumentContextPlan,
) -> BackendResult<RenderedYamlValue> {
    if node.flow {
        let mut entries = Vec::new();
        for entry in &node.entries {
            let rendered_key = if let YamlKeyValue::Complex(key) = &entry.key.value {
                render_value(
                    py,
                    prepared,
                    profile,
                    key,
                    indent,
                    validation,
                    plan,
                    CollectionRenderContext::FlowRequired,
                    ValidationMode::Text,
                )?
                .text
            } else {
                render_key(py, prepared, profile, &entry.key, validation, plan)?
            };
            let rendered_key = normalize_flow_key_text(rendered_key);
            let rendered_value = render_value(
                py,
                prepared,
                profile,
                &entry.value,
                indent,
                validation,
                plan,
                CollectionRenderContext::FlowRequired,
                ValidationMode::Text,
            )?;
            entries.push(format!("{}: {}", rendered_key, rendered_value.text));
        }
        return Ok(RenderedYamlValue::flow(
            format!("{{ {} }}", entries.join(", ")),
            node.entries.is_empty(),
        ));
    }

    let mut rendered = String::new();
    for entry in &node.entries {
        if let YamlKeyValue::Complex(key) = &entry.key.value {
            let rendered_key = render_value(
                py,
                prepared,
                profile,
                key,
                indent + 2,
                validation,
                plan,
                CollectionRenderContext::FlowRequired,
                ValidationMode::Text,
            )?;
            let rendered_value = render_value(
                py,
                prepared,
                profile,
                &entry.value,
                indent + 2,
                validation,
                plan,
                CollectionRenderContext::BlockAllowed,
                ValidationMode::Text,
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
        let key = render_key(py, prepared, profile, &entry.key, validation, plan)?;
        let rendered_value = render_value(
            py,
            prepared,
            profile,
            &entry.value,
            indent + 2,
            validation,
            plan,
            CollectionRenderContext::BlockAllowed,
            ValidationMode::Text,
        )?;
        push_rendered_value_with_prefix(
            &mut rendered,
            format!("{}{}:", " ".repeat(indent), key),
            rendered_value,
        );
    }
    Ok(RenderedYamlValue::block(rendered))
}

fn normalize_flow_key_text(rendered_key: String) -> String {
    if rendered_key.starts_with('?') && !rendered_key.starts_with("? ") {
        return format!("? {}", &rendered_key[1..]);
    }
    rendered_key
}

fn render_sequence(
    py: Python<'_>,
    prepared: &mut YamlPreparedDocument<'_>,
    profile: YamlProfile,
    node: &tstring_yaml::YamlSequenceNode,
    indent: usize,
    validation: &mut ValidationContext,
    plan: &mut DocumentContextPlan,
) -> BackendResult<RenderedYamlValue> {
    if node.flow {
        let items = node
            .items
            .iter()
            .map(|item| {
                render_value(
                    py,
                    prepared,
                    profile,
                    item,
                    indent,
                    validation,
                    plan,
                    CollectionRenderContext::FlowRequired,
                    ValidationMode::Text,
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
        let rendered_item = render_value(
            py,
            prepared,
            profile,
            item,
            indent + 2,
            validation,
            plan,
            CollectionRenderContext::BlockAllowed,
            ValidationMode::Text,
        )?;
        push_rendered_value_with_prefix(
            &mut rendered,
            format!("{}-", " ".repeat(indent)),
            rendered_item,
        );
    }
    Ok(RenderedYamlValue::block(rendered))
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

fn render_key(
    py: Python<'_>,
    prepared: &mut YamlPreparedDocument<'_>,
    profile: YamlProfile,
    node: &YamlKeyNode,
    validation: &mut ValidationContext,
    plan: &mut DocumentContextPlan,
) -> BackendResult<String> {
    match &node.value {
        YamlKeyValue::Interpolation(value) => {
            if let Some(formatted) =
                prepared.formatted_text(py, value.interpolation_index, value.span.clone())?
            {
                Ok(serde_json::to_string(&formatted).unwrap())
            } else {
                render_python_value(
                    prepared
                        .template
                        .bind_value(py, value.interpolation_index)?,
                    true,
                    Some(value.span.clone()),
                )
            }
        }
        YamlKeyValue::Scalar(YamlScalarNode::DoubleQuoted(value)) => Ok(serde_json::to_string(
            &assemble_chunks(py, prepared, &value.chunks, false)?,
        )
        .unwrap()),
        YamlKeyValue::Scalar(YamlScalarNode::SingleQuoted(value)) => Ok(format!(
            "'{}'",
            assemble_chunks(py, prepared, &value.chunks, false)?.replace('\'', "''")
        )),
        YamlKeyValue::Scalar(YamlScalarNode::Plain(value)) => {
            render_plain_scalar(py, prepared, value)
        }
        YamlKeyValue::Scalar(YamlScalarNode::Alias(node)) => {
            let name = assemble_chunks(py, prepared, &node.chunks, true)?;
            if !validation.anchors.contains_key(&name) {
                return Err(BackendError::semantic(format!(
                    "Rendered YAML contains unknown anchor {name:?} during text validation."
                )));
            }
            Ok(format!("*{name}"))
        }
        YamlKeyValue::Complex(value) => render_value(
            py,
            prepared,
            profile,
            value,
            0,
            validation,
            plan,
            CollectionRenderContext::FlowRequired,
            ValidationMode::Text,
        )
        .map(|value| value.text),
        _ => Err(BackendError::semantic(
            "YAML keys must resolve to scalar values.",
        )),
    }
}

fn render_owned_value(
    value: &YamlOwned,
    indent: usize,
    context: CollectionRenderContext,
) -> BackendResult<RenderedYamlValue> {
    match value {
        YamlOwned::Value(value) => Ok(RenderedYamlValue::inline(render_owned_scalar(value))),
        YamlOwned::Representation(value, style, tag) => {
            render_owned_representation(value, *style, tag.as_ref())
        }
        YamlOwned::Sequence(values) => render_owned_sequence(values, indent, context),
        YamlOwned::Mapping(values) => render_owned_mapping(values, indent, context),
        YamlOwned::Tagged(_, _) => Err(BackendError::semantic(
            "Python interpolation produced an unexpected tagged YAML value during text rendering.",
        )),
        YamlOwned::Alias(_) => Err(BackendError::semantic(
            "Python interpolation produced an unexpected YAML alias during text rendering.",
        )),
        YamlOwned::BadValue => Ok(RenderedYamlValue::inline("null".to_owned())),
    }
}

fn render_owned_representation(
    value: &str,
    style: ScalarStyle,
    tag: Option<&Tag>,
) -> BackendResult<RenderedYamlValue> {
    if tag.is_some() {
        return Err(BackendError::semantic(
            "Python interpolation produced an unexpected tagged scalar representation during text rendering.",
        ));
    }
    let rendered = match style {
        ScalarStyle::Plain => value.to_owned(),
        ScalarStyle::DoubleQuoted => serde_json::to_string(value).unwrap(),
        ScalarStyle::SingleQuoted => format!("'{}'", value.replace('\'', "''")),
        ScalarStyle::Literal | ScalarStyle::Folded => {
            return Err(BackendError::semantic(
                "Python interpolation produced an unexpected block scalar representation during text rendering.",
            ));
        }
    };
    Ok(RenderedYamlValue::inline(rendered))
}

fn render_owned_sequence(
    values: &[YamlOwned],
    indent: usize,
    context: CollectionRenderContext,
) -> BackendResult<RenderedYamlValue> {
    if values.is_empty() {
        return Ok(RenderedYamlValue::flow("[]".to_owned(), true));
    }
    if context == CollectionRenderContext::FlowRequired {
        let items = values
            .iter()
            .map(|item| {
                render_owned_value(item, indent, CollectionRenderContext::FlowRequired)
                    .map(|item| item.text)
            })
            .collect::<BackendResult<Vec<_>>>()?;
        return Ok(RenderedYamlValue::flow(
            format!("[ {} ]", items.join(", ")),
            false,
        ));
    }

    let mut rendered = String::new();
    for item in values {
        let rendered_item =
            render_owned_value(item, indent + 2, CollectionRenderContext::BlockAllowed)?;
        push_rendered_value_with_prefix(
            &mut rendered,
            format!("{}-", " ".repeat(indent)),
            rendered_item,
        );
    }
    Ok(RenderedYamlValue::block(rendered))
}

fn render_owned_mapping(
    values: &MappingOwned,
    indent: usize,
    context: CollectionRenderContext,
) -> BackendResult<RenderedYamlValue> {
    if values.is_empty() {
        return Ok(RenderedYamlValue::flow("{}".to_owned(), true));
    }
    if context == CollectionRenderContext::FlowRequired {
        let mut entries = Vec::new();
        for (key, value) in values {
            let rendered_key = render_owned_key(key)?;
            let rendered_value =
                render_owned_value(value, indent, CollectionRenderContext::FlowRequired)?;
            entries.push(format!("{rendered_key}: {}", rendered_value.text));
        }
        return Ok(RenderedYamlValue::flow(
            format!("{{ {} }}", entries.join(", ")),
            false,
        ));
    }

    let mut rendered = String::new();
    for (key, value) in values {
        let rendered_key = render_owned_key(key)?;
        let rendered_value =
            render_owned_value(value, indent + 2, CollectionRenderContext::BlockAllowed)?;
        push_rendered_value_with_prefix(
            &mut rendered,
            format!("{}{}:", " ".repeat(indent), rendered_key),
            rendered_value,
        );
    }
    Ok(RenderedYamlValue::block(rendered))
}

fn render_owned_key(value: &YamlOwned) -> BackendResult<String> {
    match value {
        YamlOwned::Value(value) => Ok(render_owned_scalar(value)),
        YamlOwned::Representation(value, style, tag) => {
            render_owned_representation(value, *style, tag.as_ref()).map(|value| value.text)
        }
        YamlOwned::BadValue => Ok("null".to_owned()),
        other => Err(BackendError::semantic(format!(
            "Python interpolation produced an unsupported YAML key shape during text rendering: {other:?}"
        ))),
    }
}

fn render_owned_scalar(value: &ScalarOwned) -> String {
    match value {
        ScalarOwned::Null => "null".to_owned(),
        ScalarOwned::Boolean(value) => {
            if *value {
                "true".to_owned()
            } else {
                "false".to_owned()
            }
        }
        ScalarOwned::Integer(value) => value.to_string(),
        ScalarOwned::FloatingPoint(value) => value.into_inner().to_string(),
        ScalarOwned::String(value) => serde_json::to_string(value).unwrap(),
    }
}

fn render_block_scalar(
    py: Python<'_>,
    prepared: &mut YamlPreparedDocument<'_>,
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
    let content = assemble_chunks(py, prepared, &node.chunks, false)?;
    if content.is_empty() {
        return Ok(header);
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

fn render_plain_scalar(
    py: Python<'_>,
    prepared: &mut YamlPreparedDocument<'_>,
    node: &YamlPlainScalarNode,
) -> BackendResult<String> {
    let text = assemble_chunks(py, prepared, &node.chunks, false)?
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

fn assemble_chunks(
    py: Python<'_>,
    prepared: &mut YamlPreparedDocument<'_>,
    chunks: &[YamlChunk],
    metadata: bool,
) -> BackendResult<String> {
    let mut text = String::new();
    for chunk in chunks {
        match chunk {
            YamlChunk::Text(chunk) => text.push_str(&chunk.value),
            YamlChunk::Interpolation(chunk) => {
                let fragment = if let Some(formatted) =
                    prepared.formatted_text(py, chunk.interpolation_index, chunk.span.clone())?
                {
                    formatted
                } else {
                    let value = prepared
                        .template
                        .bind_value(py, chunk.interpolation_index)?;
                    if let Ok(value) = value.extract::<String>() {
                        value
                    } else {
                        value
                            .str()
                            .map_err(|err| {
                                BackendError::unrepresentable_at(
                                    "yaml.unrepresentable.fragment",
                                    format!(
                                        "Interpolation {:?} could not be rendered as a YAML fragment: {err}",
                                        expression(prepared.template, chunk)
                                    ),
                                    Some(chunk.span.clone()),
                                )
                            })?
                            .extract::<String>()
                            .map_err(|err| {
                                BackendError::unrepresentable_at(
                                    "yaml.unrepresentable.fragment",
                                    format!(
                                        "Interpolation {:?} could not be rendered as a YAML fragment: {err}",
                                        expression(prepared.template, chunk)
                                    ),
                                    Some(chunk.span.clone()),
                                )
                            })?
                    }
                };
                if metadata && (fragment.is_empty() || fragment.chars().any(char::is_whitespace)) {
                    return Err(BackendError::unrepresentable_at(
                        "yaml.unrepresentable.metadata",
                        format!(
                            "Interpolation {:?} could not be rendered as YAML metadata because it produced whitespace or an empty value.",
                            expression(prepared.template, chunk)
                        ),
                        Some(chunk.span.clone()),
                    ));
                }
                text.push_str(&fragment);
            }
        }
    }
    Ok(text)
}

fn render_python_value(
    value: &Bound<'_, PyAny>,
    key_mode: bool,
    span: Option<tstring_syntax::SourceSpan>,
) -> BackendResult<String> {
    if value.is_none() {
        return Ok("null".to_owned());
    }
    if let Ok(value) = value.extract::<bool>() {
        return Ok(if value { "true" } else { "false" }.to_owned());
    }
    if let Some(value_text) = exact_integer_string(value).map_err(|err| {
        BackendError::unrepresentable_at(
            "yaml.unrepresentable.integer",
            format!("YAML interpolation could not be rendered as an exact integer: {err}"),
            span.clone(),
        )
    })? {
        return Ok(value_text);
    }
    if let Ok(value) = value.extract::<f64>() {
        if !value.is_finite() {
            return Err(BackendError::unrepresentable_at(
                "yaml.unrepresentable.float",
                "YAML interpolation resolved to a non-finite float, which this backend does not represent.",
                span,
            ));
        }
        return Ok(value.to_string());
    }
    if let Ok(value) = value.extract::<String>() {
        return Ok(serde_json::to_string(&value).unwrap());
    }
    if let Ok(value) = value.downcast::<PyDateTime>() {
        return value
            .call_method0("isoformat")
            .and_then(|value| value.extract::<String>())
            .map_err(|err| {
                BackendError::unrepresentable_at(
                    "yaml.unrepresentable.datetime",
                    err.to_string(),
                    span,
                )
            });
    }
    if let Ok(value) = value.downcast::<PyDate>() {
        return value
            .call_method0("isoformat")
            .and_then(|value| value.extract::<String>())
            .map_err(|err| {
                BackendError::unrepresentable_at("yaml.unrepresentable.date", err.to_string(), span)
            });
    }
    if let Ok(value) = value.downcast::<PyTime>() {
        return value
            .call_method0("isoformat")
            .and_then(|value| value.extract::<String>())
            .map_err(|err| {
                BackendError::unrepresentable_at("yaml.unrepresentable.time", err.to_string(), span)
            });
    }
    if let Ok(list) = value.downcast::<PyList>() {
        let items = list
            .iter()
            .map(|item| render_python_value(&item, false, span.clone()))
            .collect::<BackendResult<Vec<_>>>()?;
        return Ok(format!("[ {} ]", items.join(", ")));
    }
    if let Ok(dict) = value.downcast::<PyDict>() {
        let mut entries = Vec::new();
        for (key, item) in dict.iter() {
            let key_rendered = if key.is_none()
                || key.extract::<String>().is_ok()
                || exact_integer_string(&key)
                    .map(|value| value.is_some())
                    .unwrap_or(false)
                || key.extract::<f64>().is_ok()
                || key.extract::<bool>().is_ok()
            {
                render_python_value(&key, true, span.clone())?
            } else {
                return Err(BackendError::unrepresentable_at(
                    "yaml.unrepresentable.key",
                    format!(
                        "YAML key interpolation resolved to unsupported key type {}.",
                        type_name(&key)
                    ),
                    span,
                ));
            };
            entries.push(format!(
                "{key_rendered}: {}",
                render_python_value(&item, false, span.clone())?
            ));
        }
        return Ok(format!("{{ {} }}", entries.join(", ")));
    }
    if key_mode {
        let text = value
            .str()
            .map_err(|err| {
                BackendError::unrepresentable_at(
                    "yaml.unrepresentable.key",
                    err.to_string(),
                    span.clone(),
                )
            })?
            .extract::<String>()
            .map_err(|err| {
                BackendError::unrepresentable_at(
                    "yaml.unrepresentable.key",
                    err.to_string(),
                    span.clone(),
                )
            })?;
        return Ok(serde_json::to_string(&text).unwrap());
    }
    Err(BackendError::unrepresentable_at(
        "yaml.unrepresentable.value",
        format!(
            "Interpolation could not be rendered as YAML from {}.",
            type_name(value)
        ),
        span,
    ))
}

fn type_name(value: &Bound<'_, PyAny>) -> String {
    value
        .get_type()
        .name()
        .ok()
        .and_then(|name| name.extract::<String>().ok())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn expression(template: &BoundTemplate, node: &YamlInterpolationNode) -> String {
    template.expression_label(node.interpolation_index)
}
