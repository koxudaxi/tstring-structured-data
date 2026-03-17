use crate::{BoundTemplate, exact_integer_string};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use serde_json::{Number, Value};
use tstring_json::{
    JsonDocumentNode, JsonInterpolationNode, JsonKeyNode, JsonKeyValue, JsonProfile,
    JsonStringNode, JsonStringPart, JsonValueNode,
};
use tstring_syntax::{BackendError, BackendResult};

pub struct JsonRenderOutput {
    pub text: String,
    pub data: Value,
}

enum JsonSemanticValue {
    Object(Vec<JsonSemanticMember>),
    Array(Vec<JsonSemanticValue>),
    String(String),
    Literal { text: String, value: Value },
    Interpolation { text: String, value: Value },
}

struct JsonSemanticMember {
    key: JsonSemanticKey,
    value: JsonSemanticValue,
}

struct JsonSemanticKey {
    text: String,
    value: String,
}

#[derive(Clone, Default)]
enum PreparedFormattedSlot {
    #[default]
    Unresolved,
    Resolved(Option<String>),
}

struct JsonPreparedDocument<'a> {
    template: &'a BoundTemplate,
    formatted_slots: Vec<PreparedFormattedSlot>,
}

impl<'a> JsonPreparedDocument<'a> {
    fn new(template: &'a BoundTemplate) -> Self {
        Self {
            template,
            formatted_slots: vec![
                PreparedFormattedSlot::Unresolved;
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
                "Missing prepared JSON slot for interpolation index {interpolation_index}."
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
}

pub fn render_document_text(
    py: Python<'_>,
    template: &BoundTemplate,
    profile: JsonProfile,
    node: &JsonDocumentNode,
) -> BackendResult<String> {
    let mut prepared = JsonPreparedDocument::new(template);
    let artifact = execute_semantics(py, &mut prepared, profile, &node.value)?;
    Ok(render_semantic_value(&artifact))
}

pub fn render_document_data(
    py: Python<'_>,
    template: &BoundTemplate,
    profile: JsonProfile,
    node: &JsonDocumentNode,
) -> BackendResult<Value> {
    let mut prepared = JsonPreparedDocument::new(template);
    let artifact = execute_semantics(py, &mut prepared, profile, &node.value)?;
    Ok(project_native_value(&artifact))
}

pub fn render_document(
    py: Python<'_>,
    template: &BoundTemplate,
    profile: JsonProfile,
    node: &JsonDocumentNode,
) -> BackendResult<JsonRenderOutput> {
    let mut prepared = JsonPreparedDocument::new(template);
    let artifact = execute_semantics(py, &mut prepared, profile, &node.value)?;
    let text = render_semantic_value(&artifact);
    let data = project_native_value(&artifact);
    Ok(JsonRenderOutput { text, data })
}

fn execute_semantics(
    py: Python<'_>,
    prepared: &mut JsonPreparedDocument<'_>,
    profile: JsonProfile,
    node: &JsonValueNode,
) -> BackendResult<JsonSemanticValue> {
    match node {
        JsonValueNode::Object(node) => {
            let mut members = Vec::new();
            for member in &node.members {
                members.push(JsonSemanticMember {
                    key: execute_key(py, prepared, &member.key)?,
                    value: execute_semantics(py, prepared, profile, &member.value)?,
                });
            }
            Ok(JsonSemanticValue::Object(members))
        }
        JsonValueNode::Array(node) => {
            let mut items = Vec::new();
            for item in &node.items {
                items.push(execute_semantics(py, prepared, profile, item)?);
            }
            Ok(JsonSemanticValue::Array(items))
        }
        JsonValueNode::String(node) => Ok(JsonSemanticValue::String(assemble_string(
            py, prepared, node,
        )?)),
        JsonValueNode::Literal(node) => Ok(JsonSemanticValue::Literal {
            text: node.source.clone(),
            value: node.value.clone(),
        }),
        JsonValueNode::Interpolation(node) => execute_interpolation(py, prepared, profile, node),
    }
}

fn execute_key(
    py: Python<'_>,
    prepared: &mut JsonPreparedDocument<'_>,
    node: &JsonKeyNode,
) -> BackendResult<JsonSemanticKey> {
    match &node.value {
        JsonKeyValue::Interpolation(value) => {
            if let Some(formatted) =
                prepared.formatted_text(py, value.interpolation_index, value.span.clone())?
            {
                return Ok(JsonSemanticKey {
                    text: serde_json::to_string(&formatted).unwrap_or_else(|_| "\"\"".to_owned()),
                    value: formatted,
                });
            }
            let key = prepared
                .template
                .bind_value(py, value.interpolation_index)?;
            if let Ok(key) = key.extract::<String>() {
                Ok(JsonSemanticKey {
                    text: serde_json::to_string(&key).unwrap_or_else(|_| "\"\"".to_owned()),
                    value: key,
                })
            } else {
                Err(BackendError::unrepresentable_at(
                    "json.unrepresentable.key",
                    format!(
                        "Interpolation {:?} is used as a JSON object key, but resolved to {}. JSON object keys must be str.",
                        expression(prepared.template, value),
                        type_name(key)
                    ),
                    Some(value.span.clone()),
                ))
            }
        }
        JsonKeyValue::String(value) => {
            let rendered = assemble_string(py, prepared, value)?;
            Ok(JsonSemanticKey {
                text: serde_json::to_string(&rendered).unwrap_or_else(|_| "\"\"".to_owned()),
                value: rendered,
            })
        }
    }
}

fn execute_interpolation(
    py: Python<'_>,
    prepared: &mut JsonPreparedDocument<'_>,
    profile: JsonProfile,
    node: &JsonInterpolationNode,
) -> BackendResult<JsonSemanticValue> {
    if let Some(formatted) =
        prepared.formatted_text(py, node.interpolation_index, node.span.clone())?
    {
        return Ok(JsonSemanticValue::Interpolation {
            value: parse_formatted_json_value(
                profile,
                &formatted,
                &expression(prepared.template, node),
                &node.span,
            )?,
            text: formatted,
        });
    }
    let value = prepared.template.bind_value(py, node.interpolation_index)?;
    Ok(JsonSemanticValue::Interpolation {
        value: normalize_value(
            value,
            &expression(prepared.template, node),
            Some(node.span.clone()),
        )?,
        text: render_python_value(
            value,
            &expression(prepared.template, node),
            Some(node.span.clone()),
        )?,
    })
}

fn project_native_value(node: &JsonSemanticValue) -> Value {
    match node {
        JsonSemanticValue::Object(members) => {
            let mut map = serde_json::Map::new();
            for member in members {
                map.insert(
                    member.key.value.clone(),
                    project_native_value(&member.value),
                );
            }
            Value::Object(map)
        }
        JsonSemanticValue::Array(items) => {
            Value::Array(items.iter().map(project_native_value).collect())
        }
        JsonSemanticValue::String(value) => Value::String(value.clone()),
        JsonSemanticValue::Literal { value, .. }
        | JsonSemanticValue::Interpolation { value, .. } => value.clone(),
    }
}

fn render_semantic_value(node: &JsonSemanticValue) -> String {
    match node {
        JsonSemanticValue::Object(members) => {
            let mut parts = Vec::with_capacity(members.len());
            for member in members {
                parts.push(format!(
                    "{}: {}",
                    member.key.text,
                    render_semantic_value(&member.value)
                ));
            }
            format!("{{{}}}", parts.join(", "))
        }
        JsonSemanticValue::Array(items) => {
            let parts = items.iter().map(render_semantic_value).collect::<Vec<_>>();
            format!("[{}]", parts.join(", "))
        }
        JsonSemanticValue::String(value) => {
            serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
        }
        JsonSemanticValue::Literal { text, .. } | JsonSemanticValue::Interpolation { text, .. } => {
            text.clone()
        }
    }
}

fn parse_formatted_json_value(
    _profile: JsonProfile,
    formatted: &str,
    expression: &str,
    span: &tstring_syntax::SourceSpan,
) -> BackendResult<Value> {
    serde_json::from_str::<Value>(formatted).map_err(|err| {
        BackendError::parse_at(
            "json.parse",
            format!(
                "Interpolation {:?} produced invalid formatted JSON payload: {err}",
                expression
            ),
            Some(span.clone()),
        )
    })
}

fn assemble_string(
    py: Python<'_>,
    prepared: &mut JsonPreparedDocument<'_>,
    node: &JsonStringNode,
) -> BackendResult<String> {
    let mut text = String::new();
    for chunk in &node.chunks {
        match chunk {
            JsonStringPart::Chunk(chunk) => text.push_str(&chunk.value),
            JsonStringPart::Interpolation(chunk) => {
                text.push_str(&coerce_fragment(py, prepared, chunk)?)
            }
        }
    }
    Ok(text)
}

fn coerce_fragment(
    py: Python<'_>,
    prepared: &mut JsonPreparedDocument<'_>,
    node: &JsonInterpolationNode,
) -> BackendResult<String> {
    if let Some(formatted) =
        prepared.formatted_text(py, node.interpolation_index, node.span.clone())?
    {
        return Ok(formatted);
    }
    let value = prepared.template.bind_value(py, node.interpolation_index)?;
    if let Ok(value) = value.extract::<String>() {
        return Ok(value);
    }
    value
        .str()
        .map_err(|err| {
            BackendError::unrepresentable_at(
                "json.unrepresentable.fragment",
                format!(
                    "Interpolation {:?} could not be rendered as a JSON string fragment: {err}",
                    expression(prepared.template, node)
                ),
                Some(node.span.clone()),
            )
        })?
        .extract::<String>()
        .map_err(|err| {
            BackendError::unrepresentable_at(
                "json.unrepresentable.fragment",
                format!(
                    "Interpolation {:?} could not be rendered as a JSON string fragment: {err}",
                    expression(prepared.template, node)
                ),
                Some(node.span.clone()),
            )
        })
}

fn normalize_value(
    value: &Bound<'_, PyAny>,
    expression: &str,
    span: Option<tstring_syntax::SourceSpan>,
) -> BackendResult<Value> {
    if value.is_none() {
        return Ok(Value::Null);
    }
    if let Ok(value) = value.extract::<String>() {
        return Ok(Value::String(value));
    }
    if let Ok(value) = value.extract::<bool>() {
        return Ok(Value::Bool(value));
    }
    if let Some(value_text) = exact_integer_string(value).map_err(|err| {
        BackendError::unrepresentable_at(
            "json.unrepresentable.integer",
            format!(
                "Interpolation {:?} could not be rendered as an exact JSON integer: {err}",
                expression
            ),
            span.clone(),
        )
    })? {
        return serde_json::from_str::<Value>(&value_text).map_err(|err| {
            BackendError::unrepresentable_at(
                "json.unrepresentable.integer",
                format!(
                    "Interpolation {:?} resolved to integer {value_text}, but the JSON backend could not preserve it exactly: {err}",
                    expression
                ),
                span,
            )
        });
    }
    if let Ok(value) = value.extract::<f64>() {
        if !value.is_finite() {
            return Err(BackendError::unrepresentable_at(
                "json.unrepresentable.float",
                format!(
                    "Interpolation {:?} resolved to a non-finite float, which JSON does not support.",
                    expression
                ),
                span,
            ));
        }
        return Number::from_f64(value).map(Value::Number).ok_or_else(|| {
            BackendError::unrepresentable_at(
                "json.unrepresentable.float",
                format!(
                    "Interpolation {:?} resolved to a non-finite float, which JSON does not support.",
                    expression
                ),
                span,
            )
        });
    }
    if let Ok(list) = value.downcast::<PyList>() {
        let mut items = Vec::new();
        for item in list.iter() {
            items.push(normalize_value(&item, expression, span.clone())?);
        }
        return Ok(Value::Array(items));
    }
    if let Ok(dict) = value.downcast::<PyDict>() {
        let mut map = serde_json::Map::new();
        for (key, item) in dict.iter() {
            let key = key.extract::<String>().map_err(|_| {
                BackendError::unrepresentable_at(
                    "json.unrepresentable.key",
                    format!(
                        "Interpolation {:?} contains a JSON object key of type {}. JSON object keys must be str.",
                        expression,
                        type_name(&key)
                    ),
                    span.clone(),
                )
            })?;
            map.insert(key, normalize_value(&item, expression, span.clone())?);
        }
        return Ok(Value::Object(map));
    }

    Err(BackendError::unrepresentable_at(
        "json.unrepresentable.value",
        format!(
            "Interpolation {:?} could not be rendered as JSON from {}.",
            expression,
            type_name(value)
        ),
        span,
    ))
}

fn render_python_value(
    value: &Bound<'_, PyAny>,
    expression: &str,
    span: Option<tstring_syntax::SourceSpan>,
) -> BackendResult<String> {
    if value.is_none() {
        return Ok("null".to_owned());
    }
    if let Ok(value) = value.extract::<String>() {
        return serde_json::to_string(&value).map_err(|err| {
            BackendError::unrepresentable_at(
                "json.unrepresentable.value",
                format!(
                    "Interpolation {:?} could not be rendered as JSON: {err}",
                    expression
                ),
                span,
            )
        });
    }
    if let Ok(value) = value.extract::<bool>() {
        return Ok(if value { "true" } else { "false" }.to_owned());
    }
    if let Some(value_text) = exact_integer_string(value).map_err(|err| {
        BackendError::unrepresentable_at(
            "json.unrepresentable.integer",
            format!(
                "Interpolation {:?} could not be rendered as an exact JSON integer: {err}",
                expression
            ),
            span.clone(),
        )
    })? {
        return Ok(value_text);
    }
    if let Ok(value) = value.extract::<f64>() {
        if !value.is_finite() {
            return Err(BackendError::unrepresentable_at(
                "json.unrepresentable.float",
                format!(
                    "Interpolation {:?} resolved to a non-finite float, which JSON does not support.",
                    expression
                ),
                span,
            ));
        }
        return Number::from_f64(value)
            .map(|value| value.to_string())
            .ok_or_else(|| {
                BackendError::unrepresentable_at(
                    "json.unrepresentable.float",
                    format!(
                        "Interpolation {:?} resolved to a non-finite float, which JSON does not support.",
                        expression
                    ),
                    span,
                )
            });
    }
    if let Ok(list) = value.downcast::<PyList>() {
        let rendered = list
            .iter()
            .map(|item| render_python_value(&item, expression, span.clone()))
            .collect::<BackendResult<Vec<_>>>()?;
        return Ok(format!("[{}]", rendered.join(", ")));
    }
    if let Ok(dict) = value.downcast::<PyDict>() {
        let mut members = Vec::new();
        for (key, item) in dict.iter() {
            let key = key.extract::<String>().map_err(|_| {
                BackendError::unrepresentable_at(
                    "json.unrepresentable.key",
                    format!(
                        "Interpolation {:?} contains a JSON object key of type {}. JSON object keys must be str.",
                        expression,
                        type_name(&key)
                    ),
                    span.clone(),
                )
            })?;
            members.push(format!(
                "{}: {}",
                serde_json::to_string(&key).unwrap_or_else(|_| "\"\"".to_owned()),
                render_python_value(&item, expression, span.clone())?
            ));
        }
        return Ok(format!("{{{}}}", members.join(", ")));
    }
    Err(BackendError::unrepresentable_at(
        "json.unrepresentable.value",
        format!(
            "Interpolation {:?} could not be rendered as JSON from {}.",
            expression,
            type_name(value)
        ),
        span,
    ))
}

fn expression(template: &BoundTemplate, node: &JsonInterpolationNode) -> String {
    template.expression_label(node.interpolation_index)
}

fn type_name(value: &Bound<'_, PyAny>) -> String {
    value
        .get_type()
        .name()
        .ok()
        .and_then(|name| name.extract::<String>().ok())
        .unwrap_or_else(|| "unknown".to_owned())
}
