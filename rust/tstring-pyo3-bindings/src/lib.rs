use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyInt, PyString, PyTuple};
use std::ops::Deref;
use std::sync::Arc;
use tstring_syntax::{
    BackendError, Diagnostic, DiagnosticSeverity, ErrorKind, SourceSpan, TemplateInput,
    TemplateInterpolation,
};

pub mod json;
pub mod toml;
pub mod yaml;

create_exception!(tstring_bindings, TemplateError, PyException);
create_exception!(tstring_bindings, TemplateParseError, TemplateError);
create_exception!(tstring_bindings, TemplateSemanticError, TemplateError);
create_exception!(
    tstring_bindings,
    UnrepresentableValueError,
    TemplateSemanticError
);

#[derive(Clone)]
pub struct BoundTemplate {
    input: TemplateInput,
    strings: Vec<String>,
    interpolations: Vec<TemplateInterpolation>,
    values: Vec<Arc<Py<PyAny>>>,
}

impl BoundTemplate {
    #[must_use]
    pub fn new(
        input: TemplateInput,
        strings: Vec<String>,
        interpolations: Vec<TemplateInterpolation>,
        values: Vec<Arc<Py<PyAny>>>,
    ) -> Self {
        let has_formatting_metadata = interpolations.iter().any(|interpolation| {
            interpolation.conversion.is_some() || !interpolation.format_spec.is_empty()
        });
        debug_assert!(
            !has_formatting_metadata || !values.is_empty(),
            "Templates with formatting metadata must contain interpolations."
        );
        Self {
            input,
            strings,
            interpolations,
            values,
        }
    }

    #[must_use]
    pub fn input(&self) -> &TemplateInput {
        &self.input
    }

    #[must_use]
    pub fn has_interpolations(&self) -> bool {
        !self.values.is_empty()
    }

    #[must_use]
    pub fn interpolation_count(&self) -> usize {
        self.interpolations.len()
    }

    #[must_use]
    pub fn cache_key_strings(&self) -> &[String] {
        &self.strings
    }

    #[must_use]
    pub fn has_formatting_metadata(&self) -> bool {
        self.interpolations.iter().any(|interpolation| {
            interpolation.conversion.is_some() || !interpolation.format_spec.is_empty()
        })
    }

    #[must_use]
    pub fn expression_label(&self, interpolation_index: usize) -> String {
        self.interpolations
            .get(interpolation_index)
            .map(|interpolation| {
                if interpolation.expression.is_empty() {
                    format!("slot {}", interpolation_index)
                } else {
                    interpolation.expression.clone()
                }
            })
            .unwrap_or_else(|| format!("slot {}", interpolation_index))
    }

    #[must_use]
    pub fn raw_text(&self) -> String {
        self.strings.concat()
    }

    pub fn bind_value<'py>(
        &'py self,
        py: Python<'py>,
        interpolation_index: usize,
    ) -> Result<&'py Bound<'py, PyAny>, BackendError> {
        self.values
            .get(interpolation_index)
            .map(|value| value.as_ref().bind(py))
            .ok_or_else(|| {
                BackendError::semantic(format!(
                    "Missing runtime value for interpolation index {interpolation_index}."
                ))
            })
    }

    pub fn formatted_interpolation_text(
        &self,
        py: Python<'_>,
        interpolation_index: usize,
        span: Option<SourceSpan>,
    ) -> Result<Option<String>, BackendError> {
        let interpolation = self
            .interpolations
            .get(interpolation_index)
            .ok_or_else(|| {
                BackendError::semantic(format!(
                    "Missing interpolation metadata for interpolation index {interpolation_index}."
                ))
            })?;
        if interpolation.conversion.is_none() && interpolation.format_spec.is_empty() {
            return Ok(None);
        }

        let value = self.bind_value(py, interpolation_index)?;
        format_interpolation_text(py, value, interpolation)
            .map(Some)
            .map_err(|err| {
                BackendError::unrepresentable_at(
                    "tstring.unrepresentable.format",
                    format!(
                        "Interpolation {:?} could not be formatted using PEP 750 metadata: {err}",
                        self.expression_label(interpolation_index)
                    ),
                    span,
                )
            })
    }
}

impl Deref for BoundTemplate {
    type Target = TemplateInput;

    fn deref(&self) -> &Self::Target {
        &self.input
    }
}

pub fn ensure_template(
    py: Python<'_>,
    template: &Bound<'_, PyAny>,
    api_name: &str,
) -> PyResult<()> {
    let templatelib = py.import("string.templatelib")?;
    let template_type = templatelib.getattr("Template")?;
    if template.is_instance(&template_type)? {
        return Ok(());
    }

    Err(PyTypeError::new_err(format!(
        "{api_name} require a PEP 750 Template object. Got {} instead.",
        template.get_type().name()?
    )))
}

pub fn extract_template(
    py: Python<'_>,
    template: &Bound<'_, PyAny>,
    api_name: &str,
) -> PyResult<BoundTemplate> {
    ensure_template(py, template, api_name)?;

    let strings: Vec<String> = template.getattr("strings")?.extract()?;
    let interpolation_seq = template.getattr("interpolations")?;
    let iterator = interpolation_seq.try_iter()?;
    let mut interpolations = Vec::new();
    let mut values = Vec::new();

    for (interpolation_index, interpolation_any) in iterator.enumerate() {
        let interpolation = interpolation_any?;
        let expression = interpolation
            .getattr("expression")?
            .extract::<Option<String>>()?
            .unwrap_or_default();
        let conversion = interpolation
            .getattr("conversion")?
            .extract::<Option<String>>()?;
        let format_spec = interpolation
            .getattr("format_spec")?
            .extract::<Option<String>>()?
            .unwrap_or_default();

        interpolations.push(TemplateInterpolation {
            expression,
            conversion,
            format_spec,
            interpolation_index,
            raw_source: None,
        });
        values.push(Arc::new(interpolation.getattr("value")?.unbind()));
    }

    let input = TemplateInput::from_parts(strings.clone(), interpolations.clone());
    Ok(BoundTemplate::new(input, strings, interpolations, values))
}

pub fn exact_integer_string(value: &Bound<'_, PyAny>) -> PyResult<Option<String>> {
    if value.downcast::<PyInt>().is_ok() {
        return value
            .str()
            .and_then(|value| value.extract::<String>())
            .map(Some);
    }
    Ok(None)
}

fn format_interpolation_text(
    py: Python<'_>,
    value: &Bound<'_, PyAny>,
    interpolation: &TemplateInterpolation,
) -> PyResult<String> {
    let builtins = py.import("builtins")?;
    let converted = match interpolation.conversion.as_deref() {
        None => value.clone().unbind(),
        Some("r") => value.repr()?.into_any().unbind(),
        Some("s") => value.str()?.into_any().unbind(),
        Some("a") => builtins.getattr("ascii")?.call1((value,))?.unbind(),
        Some(other) => {
            return Err(PyValueError::new_err(format!(
                "Unsupported PEP 750 conversion specifier {other:?}."
            )));
        }
    };

    builtins
        .getattr("format")?
        .call1((converted.bind(py), interpolation.format_spec.as_str()))?
        .extract::<String>()
}

fn attach_backend_error_details(
    py: Python<'_>,
    err: &PyErr,
    diagnostics: &[Diagnostic],
) -> PyResult<()> {
    let top_level_code = diagnostics
        .first()
        .map(|diagnostic| diagnostic.code.as_str());
    let top_level_span = diagnostics
        .first()
        .and_then(|diagnostic| diagnostic.span.as_ref());
    let value = err.value(py);

    value.setattr(
        "code",
        top_level_code.map_or_else(
            || py.None(),
            |code| PyString::new(py, code).into_any().unbind(),
        ),
    )?;
    value.setattr(
        "span",
        top_level_span
            .map(|span| source_span_to_python(py, span))
            .transpose()?
            .unwrap_or_else(|| py.None()),
    )?;

    let rendered_diagnostics = diagnostics
        .iter()
        .map(|diagnostic| diagnostic_to_python(py, diagnostic))
        .collect::<PyResult<Vec<_>>>()?;
    value.setattr("diagnostics", PyTuple::new(py, rendered_diagnostics)?)?;
    Ok(())
}

fn diagnostic_to_python(py: Python<'_>, diagnostic: &Diagnostic) -> PyResult<Py<PyAny>> {
    let entry = PyDict::new(py);
    entry.set_item("code", diagnostic.code.as_str())?;
    entry.set_item("message", diagnostic.message.as_str())?;
    entry.set_item(
        "severity",
        match diagnostic.severity {
            DiagnosticSeverity::Error => "error",
            DiagnosticSeverity::Warning => "warning",
        },
    )?;
    entry.set_item(
        "span",
        diagnostic
            .span
            .as_ref()
            .map(|span| source_span_to_python(py, span))
            .transpose()?
            .unwrap_or_else(|| py.None()),
    )?;

    let metadata = PyDict::new(py);
    for (key, value) in &diagnostic.metadata {
        metadata.set_item(key.as_str(), value.as_str())?;
    }
    entry.set_item("metadata", metadata)?;
    Ok(entry.into_any().unbind())
}

fn source_span_to_python(py: Python<'_>, span: &SourceSpan) -> PyResult<Py<PyAny>> {
    let start = PyTuple::new(py, [span.start.token_index, span.start.offset])?;
    let end = PyTuple::new(py, [span.end.token_index, span.end.offset])?;
    Ok(
        PyTuple::new(py, [start.into_any().unbind(), end.into_any().unbind()])?
            .into_any()
            .unbind(),
    )
}

pub fn backend_error_to_py(error: BackendError) -> PyErr {
    let err = match error.kind {
        ErrorKind::Parse => TemplateParseError::new_err(error.message),
        ErrorKind::Semantic => TemplateSemanticError::new_err(error.message),
        ErrorKind::Unrepresentable => UnrepresentableValueError::new_err(error.message),
    };
    Python::with_gil(|py| {
        let _ = attach_backend_error_details(py, &err, &error.diagnostics);
        err
    })
}

#[cfg(test)]
mod tests {
    use super::{
        TemplateParseError, TemplateSemanticError, UnrepresentableValueError, backend_error_to_py,
        format_interpolation_text,
    };
    use pyo3::exceptions::PyValueError;
    use pyo3::prelude::*;
    use tstring_syntax::{BackendError, SourceSpan, TemplateInterpolation};

    #[test]
    fn backend_errors_attach_exception_metadata_without_changing_types() {
        Python::with_gil(|py| {
            let cases = [
                (
                    BackendError::parse_at("json.parse", "parse", Some(SourceSpan::point(1, 2))),
                    py.get_type::<TemplateParseError>(),
                    "json.parse",
                    true,
                ),
                (
                    BackendError::semantic_at(
                        "yaml.semantic",
                        "semantic",
                        Some(SourceSpan::point(2, 3)),
                    ),
                    py.get_type::<TemplateSemanticError>(),
                    "yaml.semantic",
                    true,
                ),
                (
                    BackendError::unrepresentable_at(
                        "toml.unrepresentable.value",
                        "unrepr",
                        Some(SourceSpan::point(3, 4)),
                    ),
                    py.get_type::<UnrepresentableValueError>(),
                    "toml.unrepresentable.value",
                    true,
                ),
                (
                    BackendError::semantic("final"),
                    py.get_type::<TemplateSemanticError>(),
                    "tstring.semantic",
                    false,
                ),
            ];

            for (error, expected_type, expected_code, expect_span) in cases {
                let err = backend_error_to_py(error);
                let value = err.value(py);
                assert!(value.is_instance(expected_type.as_any()).unwrap());
                assert_eq!(
                    value.getattr("code").unwrap().extract::<String>().unwrap(),
                    expected_code
                );
                let diagnostics = value.getattr("diagnostics").unwrap();
                assert_eq!(diagnostics.len().unwrap(), 1);
                let diagnostic = diagnostics
                    .get_item(0)
                    .unwrap()
                    .downcast_into::<pyo3::types::PyDict>()
                    .unwrap();
                assert_eq!(
                    diagnostic
                        .get_item("code")
                        .unwrap()
                        .unwrap()
                        .extract::<String>()
                        .unwrap(),
                    expected_code
                );
                assert_eq!(
                    diagnostic
                        .get_item("message")
                        .unwrap()
                        .unwrap()
                        .extract::<String>()
                        .unwrap(),
                    value.str().unwrap().extract::<String>().unwrap()
                );
                assert_eq!(
                    diagnostic
                        .get_item("severity")
                        .unwrap()
                        .unwrap()
                        .extract::<String>()
                        .unwrap(),
                    "error"
                );
                let span = value.getattr("span").unwrap();
                if expect_span {
                    assert!(!span.is_none());
                } else {
                    assert!(span.is_none());
                }
            }
        });
    }

    #[test]
    fn unsupported_conversion_specifiers_raise_value_error() {
        Python::with_gil(|py| {
            let value = 1_i32.into_pyobject(py).unwrap().into_any();
            let interpolation = TemplateInterpolation {
                expression: "value".to_owned(),
                conversion: Some("q".to_owned()),
                format_spec: String::new(),
                interpolation_index: 0,
                raw_source: None,
            };

            let err = format_interpolation_text(py, &value, &interpolation)
                .expect_err("expected unsupported conversion error");
            assert!(err.is_instance_of::<PyValueError>(py));
            assert!(
                err.to_string()
                    .contains("Unsupported PEP 750 conversion specifier")
            );
        });
    }
}
