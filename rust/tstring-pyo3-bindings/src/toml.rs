use crate::{BoundTemplate, exact_integer_string};
use pyo3::prelude::*;
use pyo3::types::{PyDate, PyDateTime, PyDict, PyList, PyTime};
use std::collections::BTreeMap;
use std::str::FromStr;
use tstring_syntax::{BackendError, BackendResult};
use tstring_toml::{
    TomlDocumentNode, TomlInterpolationNode, TomlKeyPathNode, TomlKeySegmentNode,
    TomlKeySegmentValue, TomlProfile, TomlStatementNode, TomlStringNode, TomlStringPart,
    TomlValueNode,
};

pub struct TomlRenderOutput {
    pub text: String,
    pub data: toml::Value,
}

struct TomlSemanticArtifact {
    native: toml::Table,
    statement_projections: Vec<TomlStatementProjection>,
}

enum TomlStatementProjection {
    Assignment {
        key_text: String,
        value_text: String,
    },
    TableHeader {
        key_text: String,
    },
    ArrayTableHeader {
        key_text: String,
    },
}

struct TomlValueProjection {
    native: toml::Value,
    text: String,
}

struct TomlKeyPathProjection {
    segments: Vec<String>,
    text: String,
}

#[derive(Clone, Default)]
enum PreparedFormattedSlot {
    #[default]
    Unresolved,
    Resolved(Option<String>),
}

struct TomlPreparedDocument<'a> {
    template: &'a BoundTemplate,
    formatted_slots: Vec<PreparedFormattedSlot>,
}

impl<'a> TomlPreparedDocument<'a> {
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
                "Missing prepared TOML slot for interpolation index {interpolation_index}."
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
    profile: TomlProfile,
    node: &TomlDocumentNode,
) -> BackendResult<String> {
    let mut prepared = TomlPreparedDocument::new(template);
    let artifact = execute_toml_native(py, &mut prepared, profile, node)?;
    Ok(project_toml_text(&artifact))
}

pub fn render_document_data(
    py: Python<'_>,
    template: &BoundTemplate,
    profile: TomlProfile,
    node: &TomlDocumentNode,
) -> BackendResult<toml::Value> {
    let mut prepared = TomlPreparedDocument::new(template);
    let artifact = execute_toml_native(py, &mut prepared, profile, node)?;
    Ok(toml::Value::Table(artifact.native))
}

pub fn render_document(
    py: Python<'_>,
    template: &BoundTemplate,
    profile: TomlProfile,
    node: &TomlDocumentNode,
) -> BackendResult<TomlRenderOutput> {
    let mut prepared = TomlPreparedDocument::new(template);
    let artifact = execute_toml_native(py, &mut prepared, profile, node)?;
    let text = project_toml_text(&artifact);
    let data = toml::Value::Table(artifact.native);
    Ok(TomlRenderOutput { text, data })
}

fn project_toml_text(artifact: &TomlSemanticArtifact) -> String {
    artifact
        .statement_projections
        .iter()
        .map(|statement| match statement {
            TomlStatementProjection::Assignment {
                key_text,
                value_text,
            } => format!("{key_text} = {value_text}"),
            TomlStatementProjection::TableHeader { key_text } => format!("[{key_text}]"),
            TomlStatementProjection::ArrayTableHeader { key_text } => {
                format!("[[{key_text}]]")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn execute_toml_native(
    py: Python<'_>,
    prepared: &mut TomlPreparedDocument<'_>,
    profile: TomlProfile,
    node: &TomlDocumentNode,
) -> BackendResult<TomlSemanticArtifact> {
    let mut native = toml::Table::new();
    let mut state = TableState::default();
    let mut current_table_path: Option<Vec<String>> = None;
    let mut statement_projections = Vec::with_capacity(node.statements.len());

    for statement in &node.statements {
        match statement {
            TomlStatementNode::Assignment(statement) => {
                let key_projection = resolve_key_path(py, prepared, &statement.key_path)?;
                let value_projection = resolve_value(py, prepared, profile, &statement.value)?;
                let full_path = if let Some(path) = &current_table_path {
                    join_paths(path, &key_projection.segments)
                } else {
                    key_projection.segments.clone()
                };
                let scope_depth = current_table_path.as_ref().map_or(0, Vec::len);
                assign_in_table(
                    &mut native,
                    &mut state,
                    &full_path,
                    scope_depth,
                    value_projection.native.clone(),
                )?;
                statement_projections.push(TomlStatementProjection::Assignment {
                    key_text: key_projection.text,
                    value_text: value_projection.text,
                });
            }
            TomlStatementNode::TableHeader(statement) => {
                let key_projection = resolve_key_path(py, prepared, &statement.key_path)?;
                ensure_table(&mut native, &mut state, &key_projection.segments)?;
                current_table_path = Some(key_projection.segments);
                statement_projections.push(TomlStatementProjection::TableHeader {
                    key_text: key_projection.text,
                });
            }
            TomlStatementNode::ArrayTableHeader(statement) => {
                let key_projection = resolve_key_path(py, prepared, &statement.key_path)?;
                append_array_table(&mut native, &mut state, &key_projection.segments)?;
                current_table_path = Some(key_projection.segments);
                statement_projections.push(TomlStatementProjection::ArrayTableHeader {
                    key_text: key_projection.text,
                });
            }
        }
    }

    Ok(TomlSemanticArtifact {
        native,
        statement_projections,
    })
}

fn resolve_key_path(
    py: Python<'_>,
    prepared: &mut TomlPreparedDocument<'_>,
    node: &TomlKeyPathNode,
) -> BackendResult<TomlKeyPathProjection> {
    let mut segments = Vec::with_capacity(node.segments.len());
    let mut rendered_segments = Vec::with_capacity(node.segments.len());
    for segment in &node.segments {
        segments.push(segment_to_string(py, prepared, segment)?);
        rendered_segments.push(render_key_segment(py, prepared, segment)?);
    }
    Ok(TomlKeyPathProjection {
        text: rendered_segments.join("."),
        segments,
    })
}

fn render_key_segment(
    py: Python<'_>,
    prepared: &mut TomlPreparedDocument<'_>,
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
        TomlKeySegmentValue::Interpolation(value) => {
            if let Some(formatted) =
                prepared.formatted_text(py, value.interpolation_index, value.span.clone())?
            {
                return Ok(render_basic_string(&formatted));
            }
            let rendered = prepared
                .template
                .bind_value(py, value.interpolation_index)?;
            rendered
                .extract::<String>()
                .map(|value| render_basic_string(&value))
                .map_err(|_| {
                    BackendError::unrepresentable_at(
                        "toml.unrepresentable.key",
                        format!(
                            "Interpolation {:?} is used as a TOML key, but resolved to {}. TOML keys must be str.",
                            expression(prepared.template, value),
                            type_name(rendered)
                        ),
                        Some(value.span.clone()),
                    )
                })
        }
        TomlKeySegmentValue::String(value) => {
            Ok(render_basic_string(&assemble_string(py, prepared, value)?))
        }
    }
}

fn resolve_value(
    py: Python<'_>,
    prepared: &mut TomlPreparedDocument<'_>,
    profile: TomlProfile,
    node: &TomlValueNode,
) -> BackendResult<TomlValueProjection> {
    match node {
        TomlValueNode::String(node) => {
            let text_value = assemble_string(py, prepared, node)?;
            Ok(TomlValueProjection {
                native: toml::Value::String(text_value.clone()),
                text: render_basic_string(&text_value),
            })
        }
        TomlValueNode::Literal(node) => Ok(TomlValueProjection {
            native: node.value.clone(),
            text: node.source.clone(),
        }),
        TomlValueNode::Interpolation(node) => {
            if let Some(formatted) =
                prepared.formatted_text(py, node.interpolation_index, node.span.clone())?
            {
                Ok(TomlValueProjection {
                    native: parse_formatted_toml_value(
                        profile,
                        &formatted,
                        &expression(prepared.template, node),
                        &node.span,
                    )?,
                    text: formatted,
                })
            } else {
                let bound = prepared.template.bind_value(py, node.interpolation_index)?;
                Ok(TomlValueProjection {
                    native: materialize_python_value(
                        bound,
                        &expression(prepared.template, node),
                        Some(node.span.clone()),
                    )?,
                    text: render_python_value(
                        bound,
                        &expression(prepared.template, node),
                        Some(node.span.clone()),
                    )?,
                })
            }
        }
        TomlValueNode::Array(node) => {
            let mut native_items = Vec::with_capacity(node.items.len());
            let mut text_items = Vec::with_capacity(node.items.len());
            for item in &node.items {
                let projection = resolve_value(py, prepared, profile, item)?;
                native_items.push(projection.native);
                text_items.push(projection.text);
            }
            Ok(TomlValueProjection {
                native: toml::Value::Array(native_items),
                text: format!("[{}]", text_items.join(", ")),
            })
        }
        TomlValueNode::InlineTable(node) => {
            let mut table = toml::Table::new();
            let mut state = TableState::default();
            let mut entries = Vec::with_capacity(node.entries.len());
            for entry in &node.entries {
                let key_projection = resolve_key_path(py, prepared, &entry.key_path)?;
                let value_projection = resolve_value(py, prepared, profile, &entry.value)?;
                assign_in_table(
                    &mut table,
                    &mut state,
                    &key_projection.segments,
                    0,
                    value_projection.native.clone(),
                )?;
                entries.push(format!(
                    "{} = {}",
                    key_projection.text, value_projection.text
                ));
            }
            Ok(TomlValueProjection {
                native: toml::Value::Table(table),
                text: if entries.is_empty() {
                    "{}".to_owned()
                } else {
                    format!("{{ {} }}", entries.join(", "))
                },
            })
        }
    }
}

fn assemble_string(
    py: Python<'_>,
    prepared: &mut TomlPreparedDocument<'_>,
    node: &TomlStringNode,
) -> BackendResult<String> {
    let mut text = String::new();
    for chunk in &node.chunks {
        match chunk {
            TomlStringPart::Chunk(chunk) => text.push_str(&chunk.value),
            TomlStringPart::Interpolation(chunk) => {
                if let Some(formatted) =
                    prepared.formatted_text(py, chunk.interpolation_index, chunk.span.clone())?
                {
                    text.push_str(&formatted);
                    continue;
                }
                let value = prepared
                    .template
                    .bind_value(py, chunk.interpolation_index)?;
                if value.is_none() {
                    return Err(BackendError::unrepresentable_at(
                        "toml.unrepresentable.fragment",
                        format!(
                            "Interpolation {:?} cannot be rendered inside a TOML string fragment because TOML does not support null values.",
                            expression(prepared.template, chunk)
                        ),
                        Some(chunk.span.clone()),
                    ));
                }
                if let Ok(value) = value.extract::<String>() {
                    text.push_str(&value);
                } else {
                    let rendered = value.str().map_err(|err| {
                        BackendError::unrepresentable_at(
                            "toml.unrepresentable.fragment",
                            format!(
                                "Interpolation {:?} could not be rendered as a TOML string fragment: {err}",
                                expression(prepared.template, chunk)
                            ),
                            Some(chunk.span.clone()),
                        )
                    })?;
                    text.push_str(&rendered.extract::<String>().map_err(|err| {
                        BackendError::unrepresentable_at(
                            "toml.unrepresentable.fragment",
                            format!(
                                "Interpolation {:?} could not be rendered as a TOML string fragment: {err}",
                                expression(prepared.template, chunk)
                            ),
                            Some(chunk.span.clone()),
                        )
                    })?);
                }
            }
        }
    }
    Ok(text)
}

fn render_python_value(
    value: &Bound<'_, PyAny>,
    expression: &str,
    span: Option<tstring_syntax::SourceSpan>,
) -> BackendResult<String> {
    if value.is_none() {
        return Err(BackendError::unrepresentable_at(
            "toml.unrepresentable.null",
            format!(
                "Interpolation {:?} resolved to None, but TOML has no null value.",
                expression
            ),
            span,
        ));
    }
    if let Ok(value) = value.extract::<String>() {
        return Ok(render_basic_string(&value));
    }
    if let Ok(value) = value.extract::<bool>() {
        return Ok(if value { "true" } else { "false" }.to_owned());
    }
    if let Some(value_text) = exact_integer_string(value).map_err(|err| {
        BackendError::unrepresentable_at(
            "toml.unrepresentable.integer",
            format!(
                "Interpolation {:?} could not be rendered as an exact TOML integer: {err}",
                expression
            ),
            span.clone(),
        )
    })? {
        let parsed = value_text.parse::<i64>().map_err(|_| {
            BackendError::unrepresentable_at(
                "toml.unrepresentable.integer",
                format!(
                    "Interpolation {:?} resolved to integer {value_text}, but TOML integers must fit in the signed 64-bit range.",
                    expression
                ),
                span.clone(),
            )
        })?;
        return Ok(parsed.to_string());
    }
    if let Ok(value) = value.extract::<f64>() {
        if !value.is_finite() {
            if value.is_nan() {
                return Ok("nan".to_owned());
            }
            return Ok(if value.is_sign_negative() {
                "-inf"
            } else {
                "inf"
            }
            .to_owned());
        }
        return Ok(value.to_string());
    }
    if let Ok(value) = value.downcast::<PyDateTime>() {
        return value
            .call_method0("isoformat")
            .and_then(|value| value.extract::<String>())
            .map_err(|err| {
                BackendError::unrepresentable_at(
                    "toml.unrepresentable.datetime",
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
                BackendError::unrepresentable_at("toml.unrepresentable.date", err.to_string(), span)
            });
    }
    if let Ok(value) = value.downcast::<PyTime>() {
        if !value
            .getattr("tzinfo")
            .map_err(|err| {
                BackendError::unrepresentable_at(
                    "toml.unrepresentable.time",
                    err.to_string(),
                    span.clone(),
                )
            })?
            .is_none()
        {
            return Err(BackendError::unrepresentable_at(
                "toml.unrepresentable.time",
                format!(
                    "Interpolation {:?} resolved to a time with timezone information, which TOML does not support.",
                    expression
                ),
                span,
            ));
        }
        return value
            .call_method0("isoformat")
            .and_then(|value| value.extract::<String>())
            .map_err(|err| {
                BackendError::unrepresentable_at("toml.unrepresentable.time", err.to_string(), span)
            });
    }
    if let Ok(list) = value.downcast::<PyList>() {
        let mut items = Vec::new();
        for item in list.iter() {
            items.push(render_python_value(&item, expression, span.clone())?);
        }
        return Ok(format!("[{}]", items.join(", ")));
    }
    if let Ok(dict) = value.downcast::<PyDict>() {
        let mut entries = Vec::new();
        for (key, item) in dict.iter() {
            let key = key.extract::<String>().map_err(|_| {
                BackendError::unrepresentable_at(
                    "toml.unrepresentable.key",
                    format!(
                        "Interpolation {:?} contains a TOML key of type {}. TOML keys must be str.",
                        expression,
                        type_name(&key)
                    ),
                    span.clone(),
                )
            })?;
            entries.push(format!(
                "{} = {}",
                render_basic_string(&key),
                render_python_value(&item, expression, span.clone())?
            ));
        }
        return Ok(if entries.is_empty() {
            "{}".to_owned()
        } else {
            format!("{{ {} }}", entries.join(", "))
        });
    }

    Err(BackendError::unrepresentable_at(
        "toml.unrepresentable.value",
        format!(
            "Interpolation {:?} could not be rendered as TOML from {}.",
            expression,
            type_name(value)
        ),
        span,
    ))
}

#[derive(Default)]
struct TableState {
    children: BTreeMap<String, ValueState>,
    frozen: bool,
    explicit: bool,
    header_reopenable: bool,
}

struct ArrayState {
    items: Vec<ValueState>,
    array_of_tables: bool,
}

enum ValueState {
    Scalar,
    Table(TableState),
    Array(ArrayState),
}

fn join_paths(left: &[String], right: &[String]) -> Vec<String> {
    let mut path = left.to_vec();
    path.extend(right.iter().cloned());
    path
}

fn segment_to_string(
    py: Python<'_>,
    prepared: &mut TomlPreparedDocument<'_>,
    node: &TomlKeySegmentNode,
) -> BackendResult<String> {
    match &node.value {
        TomlKeySegmentValue::Bare(value) => Ok(value.clone()),
        TomlKeySegmentValue::Interpolation(value) => {
            if let Some(formatted) =
                prepared.formatted_text(py, value.interpolation_index, value.span.clone())?
            {
                return Ok(formatted);
            }
            prepared
                .template
                .bind_value(py, value.interpolation_index)?
                .extract::<String>()
                .map_err(|_| {
                    BackendError::unrepresentable_at(
                        "toml.unrepresentable.key",
                        format!(
                            "Interpolation {:?} is used as a TOML key, but resolved to {}. TOML keys must be str.",
                            expression(prepared.template, value),
                            type_name(
                                prepared
                                    .template
                                    .bind_value(py, value.interpolation_index)
                                    .expect("interpolation value")
                            )
                        ),
                        Some(value.span.clone()),
                    )
                })
        }
        TomlKeySegmentValue::String(value) => assemble_string(py, prepared, value),
    }
}

fn type_name(value: &Bound<'_, PyAny>) -> String {
    value
        .get_type()
        .name()
        .ok()
        .and_then(|name| name.extract::<String>().ok())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn assign_in_table(
    base: &mut toml::Table,
    state: &mut TableState,
    path: &[String],
    scope_depth: usize,
    value: toml::Value,
) -> BackendResult<()> {
    let mut cursor = base;
    let mut cursor_state = state;
    for (index, segment) in path[..path.len() - 1].iter().enumerate() {
        let (next_table, next_state) = descend_table(
            cursor,
            cursor_state,
            segment,
            CreateMode::Assignment {
                in_current_scope: index < scope_depth,
            },
        )?;
        cursor = next_table;
        cursor_state = next_state;
    }
    let leaf = path[path.len() - 1].clone();
    if cursor_state.children.contains_key(&leaf) {
        return Err(BackendError::semantic(format!(
            "Duplicate TOML key {leaf:?} cannot be assigned more than once."
        )));
    }
    cursor_state
        .children
        .insert(leaf.clone(), state_from_value(&value, true));
    cursor.insert(leaf, value);
    Ok(())
}

fn ensure_table(
    root: &mut toml::Table,
    state: &mut TableState,
    path: &[String],
) -> BackendResult<()> {
    let mut cursor = root;
    let mut cursor_state = state;
    for segment in &path[..path.len() - 1] {
        let (next_table, next_state) =
            descend_table(cursor, cursor_state, segment, CreateMode::Header)?;
        cursor = next_table;
        cursor_state = next_state;
    }

    let leaf = path[path.len() - 1].clone();
    match cursor_state.children.get(&leaf) {
        None => {
            cursor.insert(leaf.clone(), toml::Value::Table(toml::Table::new()));
            cursor_state.children.insert(
                leaf,
                ValueState::Table(TableState {
                    explicit: true,
                    header_reopenable: true,
                    ..TableState::default()
                }),
            );
            Ok(())
        }
        Some(ValueState::Table(existing))
            if !existing.frozen && !existing.explicit && existing.header_reopenable =>
        {
            let Some(ValueState::Table(existing)) = cursor_state.children.get_mut(&leaf) else {
                unreachable!();
            };
            existing.explicit = true;
            Ok(())
        }
        Some(ValueState::Table(_)) => Err(BackendError::semantic(format!(
            "Duplicate TOML table {:?} cannot be defined more than once.",
            path.join(".")
        ))),
        Some(ValueState::Array(ArrayState {
            array_of_tables: true,
            ..
        })) => Err(BackendError::semantic(format!(
            "Conflicting TOML table {:?} cannot redefine an array-of-tables.",
            path.join(".")
        ))),
        _ => Err(BackendError::semantic("Conflicting TOML table structure.")),
    }
}

fn append_array_table(
    root: &mut toml::Table,
    state: &mut TableState,
    path: &[String],
) -> BackendResult<()> {
    let mut cursor = root;
    let mut cursor_state = state;
    for segment in &path[..path.len() - 1] {
        let (next_table, next_state) =
            descend_table(cursor, cursor_state, segment, CreateMode::Header)?;
        cursor = next_table;
        cursor_state = next_state;
    }
    let leaf = path[path.len() - 1].clone();
    if !cursor_state.children.contains_key(&leaf) {
        cursor.insert(leaf.clone(), toml::Value::Array(Vec::new()));
        cursor_state.children.insert(
            leaf.clone(),
            ValueState::Array(ArrayState {
                items: Vec::new(),
                array_of_tables: true,
            }),
        );
    }
    match (cursor.get_mut(&leaf), cursor_state.children.get_mut(&leaf)) {
        (Some(toml::Value::Array(array)), Some(ValueState::Array(array_state)))
            if array_state.array_of_tables =>
        {
            array.push(toml::Value::Table(toml::Table::new()));
            array_state.items.push(ValueState::Table(TableState {
                explicit: true,
                header_reopenable: true,
                ..TableState::default()
            }));
            Ok(())
        }
        (Some(toml::Value::Table(_)), Some(ValueState::Table(_))) => {
            Err(BackendError::semantic(format!(
                "Conflicting TOML array-of-tables {:?} cannot redefine a table.",
                path.join(".")
            )))
        }
        _ => Err(BackendError::semantic(
            "Conflicting TOML array-of-table structure.",
        )),
    }
}

fn descend_table<'a>(
    table: &'a mut toml::Table,
    state: &'a mut TableState,
    segment: &str,
    create_mode: CreateMode,
) -> BackendResult<(&'a mut toml::Table, &'a mut TableState)> {
    if !state.children.contains_key(segment) {
        table.insert(segment.to_owned(), toml::Value::Table(toml::Table::new()));
        state.children.insert(
            segment.to_owned(),
            ValueState::Table(TableState {
                header_reopenable: matches!(create_mode, CreateMode::Header),
                ..TableState::default()
            }),
        );
    }

    match (table.get_mut(segment), state.children.get_mut(segment)) {
        (Some(toml::Value::Table(next_table)), Some(ValueState::Table(next_state))) => {
            if next_state.frozen {
                return Err(BackendError::semantic("Conflicting TOML table structure."));
            }
            if let CreateMode::Assignment { in_current_scope } = create_mode
                && next_state.explicit
                && !in_current_scope
            {
                return Err(BackendError::semantic("Conflicting TOML table structure."));
            }
            Ok((next_table, next_state))
        }
        (Some(toml::Value::Array(array)), Some(ValueState::Array(array_state)))
            if array_state.array_of_tables =>
        {
            if matches!(
                create_mode,
                CreateMode::Assignment {
                    in_current_scope: false
                }
            ) {
                return Err(BackendError::semantic("Conflicting TOML table structure."));
            }
            let Some(toml::Value::Table(next_table)) = array.last_mut() else {
                return Err(BackendError::semantic("Invalid TOML array-of-table state."));
            };
            let Some(ValueState::Table(next_state)) = array_state.items.last_mut() else {
                return Err(BackendError::semantic("Invalid TOML array-of-table state."));
            };
            if next_state.frozen {
                return Err(BackendError::semantic("Conflicting TOML table structure."));
            }
            Ok((next_table, next_state))
        }
        _ => Err(BackendError::semantic("Conflicting TOML table structure.")),
    }
}

#[derive(Clone, Copy)]
enum CreateMode {
    Assignment { in_current_scope: bool },
    Header,
}

fn state_from_value(value: &toml::Value, frozen: bool) -> ValueState {
    match value {
        toml::Value::Table(table) => ValueState::Table(TableState {
            children: table
                .iter()
                .map(|(key, item)| (key.clone(), state_from_value(item, true)))
                .collect(),
            frozen,
            explicit: false,
            header_reopenable: false,
        }),
        toml::Value::Array(items) => ValueState::Array(ArrayState {
            items: items
                .iter()
                .map(|item| state_from_value(item, true))
                .collect(),
            array_of_tables: false,
        }),
        _ => ValueState::Scalar,
    }
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

fn expression(template: &BoundTemplate, node: &TomlInterpolationNode) -> String {
    template.expression_label(node.interpolation_index)
}

fn parse_formatted_toml_value(
    profile: TomlProfile,
    formatted: &str,
    expression: &str,
    span: &tstring_syntax::SourceSpan,
) -> BackendResult<toml::Value> {
    tstring_toml::materialize_value_source(profile, formatted).map_err(|message| {
        BackendError::parse_at(
            "toml.parse",
            format!(
                "Interpolation {:?} produced invalid formatted TOML payload: {message}",
                expression
            ),
            Some(span.clone()),
        )
    })
}

fn materialize_python_value(
    value: &Bound<'_, PyAny>,
    expression: &str,
    span: Option<tstring_syntax::SourceSpan>,
) -> BackendResult<toml::Value> {
    if value.is_none() {
        return Err(BackendError::unrepresentable_at(
            "toml.unrepresentable.null",
            format!(
                "Interpolation {:?} resolved to None, but TOML has no null value.",
                expression
            ),
            span,
        ));
    }
    if let Ok(value) = value.extract::<String>() {
        return Ok(toml::Value::String(value));
    }
    if let Ok(value) = value.extract::<bool>() {
        return Ok(toml::Value::Boolean(value));
    }
    if let Some(value_text) = exact_integer_string(value).map_err(|err| {
        BackendError::unrepresentable_at(
            "toml.unrepresentable.integer",
            format!(
                "Interpolation {:?} could not be rendered as an exact TOML integer: {err}",
                expression
            ),
            span.clone(),
        )
    })? {
        let parsed = value_text.parse::<i64>().map_err(|_| {
            BackendError::unrepresentable_at(
                "toml.unrepresentable.integer",
                format!(
                    "Interpolation {:?} resolved to integer {value_text}, but TOML integers must fit in the signed 64-bit range.",
                    expression
                ),
                span.clone(),
            )
        })?;
        return Ok(toml::Value::Integer(parsed));
    }
    if let Ok(value) = value.extract::<f64>() {
        return Ok(toml::Value::Float(value));
    }
    if let Ok(value) = value.downcast::<PyDateTime>() {
        let rendered = value
            .call_method0("isoformat")
            .and_then(|value| value.extract::<String>())
            .map_err(|err| {
                BackendError::unrepresentable_at(
                    "toml.unrepresentable.datetime",
                    err.to_string(),
                    span.clone(),
                )
            })?;
        let datetime = toml::value::Datetime::from_str(&rendered).map_err(|err| {
            BackendError::unrepresentable_at(
                "toml.unrepresentable.datetime",
                err.to_string(),
                span.clone(),
            )
        })?;
        return Ok(toml::Value::Datetime(datetime));
    }
    if let Ok(value) = value.downcast::<PyDate>() {
        let rendered = value
            .call_method0("isoformat")
            .and_then(|value| value.extract::<String>())
            .map_err(|err| {
                BackendError::unrepresentable_at(
                    "toml.unrepresentable.date",
                    err.to_string(),
                    span.clone(),
                )
            })?;
        let datetime = toml::value::Datetime::from_str(&rendered).map_err(|err| {
            BackendError::unrepresentable_at(
                "toml.unrepresentable.date",
                err.to_string(),
                span.clone(),
            )
        })?;
        return Ok(toml::Value::Datetime(datetime));
    }
    if let Ok(value) = value.downcast::<PyTime>() {
        if !value
            .getattr("tzinfo")
            .map_err(|err| {
                BackendError::unrepresentable_at(
                    "toml.unrepresentable.time",
                    err.to_string(),
                    span.clone(),
                )
            })?
            .is_none()
        {
            return Err(BackendError::unrepresentable_at(
                "toml.unrepresentable.time",
                format!(
                    "Interpolation {:?} resolved to a time with timezone information, which TOML does not support.",
                    expression
                ),
                span,
            ));
        }
        let rendered = value
            .call_method0("isoformat")
            .and_then(|value| value.extract::<String>())
            .map_err(|err| {
                BackendError::unrepresentable_at(
                    "toml.unrepresentable.time",
                    err.to_string(),
                    span.clone(),
                )
            })?;
        let datetime = toml::value::Datetime::from_str(&rendered).map_err(|err| {
            BackendError::unrepresentable_at(
                "toml.unrepresentable.time",
                err.to_string(),
                span.clone(),
            )
        })?;
        return Ok(toml::Value::Datetime(datetime));
    }
    if let Ok(list) = value.downcast::<PyList>() {
        let mut items = Vec::new();
        for item in list.iter() {
            items.push(materialize_python_value(&item, expression, span.clone())?);
        }
        return Ok(toml::Value::Array(items));
    }
    if let Ok(dict) = value.downcast::<PyDict>() {
        let mut table = toml::Table::new();
        let mut state = TableState::default();
        for (key, item) in dict.iter() {
            let key = key.extract::<String>().map_err(|_| {
                BackendError::unrepresentable_at(
                    "toml.unrepresentable.key",
                    format!(
                        "Interpolation {:?} contains a TOML key of type {}. TOML keys must be str.",
                        expression,
                        type_name(&key)
                    ),
                    span.clone(),
                )
            })?;
            assign_in_table(
                &mut table,
                &mut state,
                &[key],
                0,
                materialize_python_value(&item, expression, span.clone())?,
            )?;
        }
        return Ok(toml::Value::Table(table));
    }

    Err(BackendError::unrepresentable_at(
        "toml.unrepresentable.value",
        format!(
            "Interpolation {:?} could not be rendered as TOML from {}.",
            expression,
            type_name(value)
        ),
        span,
    ))
}
