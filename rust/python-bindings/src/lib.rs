use pyo3::Bound;
use pyo3::conversion::IntoPyObjectExt;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyModule, PyString, PyTuple};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use tstring_json::{JsonDocumentNode, JsonProfile};
use tstring_pyo3_bindings::{
    BoundTemplate, TemplateError, TemplateParseError, TemplateSemanticError,
    UnrepresentableValueError, backend_error_to_py, extract_template,
};
use tstring_syntax::{
    NormalizedDocument, NormalizedFloat, NormalizedKey, NormalizedStream, NormalizedTemporal,
    NormalizedValue,
};
use tstring_toml::{TomlDocumentNode, TomlProfile};
use tstring_yaml::{YamlProfile, YamlStreamNode};

const PARSE_CACHE_CAPACITY: usize = 256;
const CONTRACT_VERSION: u32 = 1;
const CONTRACT_SYMBOLS: &[&str] = &[
    "TemplateError",
    "TemplateParseError",
    "TemplateSemanticError",
    "UnrepresentableValueError",
    "render_json",
    "render_json_text",
    "_render_json_result_payload",
    "render_toml",
    "render_toml_text",
    "_render_toml_result_payload",
    "render_yaml",
    "render_yaml_text",
    "_render_yaml_result_payload",
];

type CacheKey = (String, Vec<String>);

struct ParseCache<T> {
    capacity: usize,
    state: Mutex<ParseCacheState<T>>,
}

struct ParseCacheState<T> {
    entries: HashMap<CacheKey, Arc<T>>,
    order: VecDeque<CacheKey>,
}

impl<T> ParseCache<T> {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            state: Mutex::new(ParseCacheState {
                entries: HashMap::new(),
                order: VecDeque::new(),
            }),
        }
    }

    fn get_or_try_insert_with<E, F>(&self, key: &CacheKey, build: F) -> Result<Arc<T>, E>
    where
        F: FnOnce() -> Result<T, E>,
    {
        if let Some(value) = self.get(key) {
            return Ok(value);
        }

        let parsed = Arc::new(build()?);
        let key = key.clone();
        let mut state = self.lock_state();
        if let Some(value) = state.entries.get(&key).cloned() {
            Self::touch_key(&mut state, &key);
            return Ok(value);
        }
        self.insert_locked(&mut state, key, Arc::clone(&parsed));
        Ok(parsed)
    }

    fn get(&self, key: &CacheKey) -> Option<Arc<T>> {
        let key = key.clone();
        let mut state = self.lock_state();
        let value = state.entries.get(&key).cloned();
        if value.is_some() {
            Self::touch_key(&mut state, &key);
        }
        value
    }

    fn insert_locked(&self, state: &mut ParseCacheState<T>, key: CacheKey, value: Arc<T>) {
        if state.entries.contains_key(&key) {
            state.entries.insert(key.clone(), value);
            Self::touch_key(state, &key);
            return;
        }
        if state.entries.len() == self.capacity {
            while let Some(oldest) = state.order.pop_front() {
                if state.entries.remove(&oldest).is_some() {
                    break;
                }
            }
        }
        state.order.push_back(key.clone());
        state.entries.insert(key, value);
    }

    fn touch_key(state: &mut ParseCacheState<T>, key: &CacheKey) {
        if let Some(index) = state.order.iter().position(|existing| existing == key) {
            state.order.remove(index);
        }
        state.order.push_back(key.clone());
    }

    fn lock_state(&self) -> MutexGuard<'_, ParseCacheState<T>> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.lock_state().entries.len()
    }
}

fn json_parse_cache() -> &'static ParseCache<JsonDocumentNode> {
    static CACHE: OnceLock<ParseCache<JsonDocumentNode>> = OnceLock::new();
    CACHE.get_or_init(|| ParseCache::new(PARSE_CACHE_CAPACITY))
}

fn toml_parse_cache() -> &'static ParseCache<TomlDocumentNode> {
    static CACHE: OnceLock<ParseCache<TomlDocumentNode>> = OnceLock::new();
    CACHE.get_or_init(|| ParseCache::new(PARSE_CACHE_CAPACITY))
}

fn yaml_parse_cache() -> &'static ParseCache<YamlStreamNode> {
    static CACHE: OnceLock<ParseCache<YamlStreamNode>> = OnceLock::new();
    CACHE.get_or_init(|| ParseCache::new(PARSE_CACHE_CAPACITY))
}

fn template_cache_key(template: &BoundTemplate, profile: &str) -> CacheKey {
    (profile.to_owned(), template.cache_key_strings().to_vec())
}

fn parse_json_profile(profile: &str) -> PyResult<JsonProfile> {
    profile.parse().map_err(PyValueError::new_err)
}

fn parse_toml_profile(profile: &str) -> PyResult<TomlProfile> {
    profile.parse().map_err(PyValueError::new_err)
}

fn parse_yaml_profile(profile: &str) -> PyResult<YamlProfile> {
    profile.parse().map_err(PyValueError::new_err)
}

fn parse_json_template(
    template: &BoundTemplate,
    profile: JsonProfile,
) -> PyResult<Arc<JsonDocumentNode>> {
    json_parse_cache()
        .get_or_try_insert_with(&template_cache_key(template, profile.as_str()), || {
            tstring_json::parse_template_with_profile(template.input(), profile)
        })
        .map_err(backend_error_to_py)
}

fn parse_toml_template(
    template: &BoundTemplate,
    profile: TomlProfile,
) -> PyResult<Arc<TomlDocumentNode>> {
    toml_parse_cache()
        .get_or_try_insert_with(&template_cache_key(template, profile.as_str()), || {
            tstring_toml::parse_template_with_profile(template.input(), profile)
        })
        .map_err(backend_error_to_py)
}

fn parse_yaml_template(
    template: &BoundTemplate,
    profile: YamlProfile,
) -> PyResult<Arc<YamlStreamNode>> {
    yaml_parse_cache()
        .get_or_try_insert_with(&template_cache_key(template, profile.as_str()), || {
            tstring_yaml::parse_template_with_profile(template.input(), profile)
        })
        .map_err(backend_error_to_py)
}

fn render_json_for_profile(
    py: Python<'_>,
    template: &Bound<'_, PyAny>,
    profile: JsonProfile,
) -> PyResult<Py<PyAny>> {
    let template = extract_template(py, template, "render_json/render_json_text")?;
    let node = parse_json_template(&template, profile)?;
    let rendered =
        tstring_pyo3_bindings::json::render_document_data(py, &template, profile, node.as_ref())
            .map_err(backend_error_to_py)?;
    let normalized = tstring_json::normalize_document_with_profile(&rendered, profile)
        .map_err(backend_error_to_py)?;
    normalized_stream_to_python(py, &normalized)
}

fn render_json(py: Python<'_>, template: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    render_json_for_profile(py, template, JsonProfile::default())
}

#[pyfunction(name = "render_json", signature = (template, profile = "rfc8259"))]
fn render_json_py(
    py: Python<'_>,
    template: &Bound<'_, PyAny>,
    profile: &str,
) -> PyResult<Py<PyAny>> {
    render_json_for_profile(py, template, parse_json_profile(profile)?)
}

fn render_json_text_for_profile(
    py: Python<'_>,
    template: &Bound<'_, PyAny>,
    profile: JsonProfile,
) -> PyResult<String> {
    let template = extract_template(py, template, "render_json/render_json_text")?;
    let node = parse_json_template(&template, profile)?;
    tstring_pyo3_bindings::json::render_document_text(py, &template, profile, node.as_ref())
        .map_err(backend_error_to_py)
}

fn render_json_text(py: Python<'_>, template: &Bound<'_, PyAny>) -> PyResult<String> {
    render_json_text_for_profile(py, template, JsonProfile::default())
}

#[pyfunction(name = "render_json_text", signature = (template, profile = "rfc8259"))]
fn render_json_text_py(
    py: Python<'_>,
    template: &Bound<'_, PyAny>,
    profile: &str,
) -> PyResult<String> {
    render_json_text_for_profile(py, template, parse_json_profile(profile)?)
}

fn render_json_result_payload_for_profile(
    py: Python<'_>,
    template: &Bound<'_, PyAny>,
    profile: JsonProfile,
) -> PyResult<(String, Py<PyAny>)> {
    let template = extract_template(py, template, "render_result")?;
    let node = parse_json_template(&template, profile)?;
    let rendered =
        tstring_pyo3_bindings::json::render_document(py, &template, profile, node.as_ref())
            .map_err(backend_error_to_py)?;
    let normalized = tstring_json::normalize_document_with_profile(&rendered.data, profile)
        .map_err(backend_error_to_py)?;
    let data = normalized_stream_to_python(py, &normalized)?;
    Ok((rendered.text, data))
}

fn _render_json_result_payload(
    py: Python<'_>,
    template: &Bound<'_, PyAny>,
) -> PyResult<(String, Py<PyAny>)> {
    render_json_result_payload_for_profile(py, template, JsonProfile::default())
}

#[pyfunction(name = "_render_json_result_payload", signature = (template, profile = "rfc8259"))]
fn render_json_result_payload_py(
    py: Python<'_>,
    template: &Bound<'_, PyAny>,
    profile: &str,
) -> PyResult<(String, Py<PyAny>)> {
    render_json_result_payload_for_profile(py, template, parse_json_profile(profile)?)
}

fn render_toml_for_profile(
    py: Python<'_>,
    template: &Bound<'_, PyAny>,
    profile: TomlProfile,
) -> PyResult<Py<PyAny>> {
    let template = extract_template(py, template, "render_toml/render_toml_text")?;
    let node = parse_toml_template(&template, profile)?;
    let rendered =
        tstring_pyo3_bindings::toml::render_document_data(py, &template, profile, node.as_ref())
            .map_err(backend_error_to_py)?;
    let normalized = tstring_toml::normalize_document_with_profile(&rendered, profile)
        .map_err(backend_error_to_py)?;
    normalized_stream_to_python(py, &normalized)
}

fn render_toml(py: Python<'_>, template: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    render_toml_for_profile(py, template, TomlProfile::default())
}

#[pyfunction(name = "render_toml", signature = (template, profile = "1.1"))]
fn render_toml_py(
    py: Python<'_>,
    template: &Bound<'_, PyAny>,
    profile: &str,
) -> PyResult<Py<PyAny>> {
    render_toml_for_profile(py, template, parse_toml_profile(profile)?)
}

fn render_toml_text_for_profile(
    py: Python<'_>,
    template: &Bound<'_, PyAny>,
    profile: TomlProfile,
) -> PyResult<String> {
    let template = extract_template(py, template, "render_toml/render_toml_text")?;
    let node = parse_toml_template(&template, profile)?;
    tstring_pyo3_bindings::toml::render_document_text(py, &template, profile, node.as_ref())
        .map_err(backend_error_to_py)
}

fn render_toml_text(py: Python<'_>, template: &Bound<'_, PyAny>) -> PyResult<String> {
    render_toml_text_for_profile(py, template, TomlProfile::default())
}

#[pyfunction(name = "render_toml_text", signature = (template, profile = "1.1"))]
fn render_toml_text_py(
    py: Python<'_>,
    template: &Bound<'_, PyAny>,
    profile: &str,
) -> PyResult<String> {
    render_toml_text_for_profile(py, template, parse_toml_profile(profile)?)
}

fn render_toml_result_payload_for_profile(
    py: Python<'_>,
    template: &Bound<'_, PyAny>,
    profile: TomlProfile,
) -> PyResult<(String, Py<PyAny>)> {
    let template = extract_template(py, template, "render_result")?;
    let node = parse_toml_template(&template, profile)?;
    let rendered =
        tstring_pyo3_bindings::toml::render_document(py, &template, profile, node.as_ref())
            .map_err(backend_error_to_py)?;
    let normalized = tstring_toml::normalize_document_with_profile(&rendered.data, profile)
        .map_err(backend_error_to_py)?;
    let data = normalized_stream_to_python(py, &normalized)?;
    Ok((rendered.text, data))
}

fn _render_toml_result_payload(
    py: Python<'_>,
    template: &Bound<'_, PyAny>,
) -> PyResult<(String, Py<PyAny>)> {
    render_toml_result_payload_for_profile(py, template, TomlProfile::default())
}

#[pyfunction(name = "_render_toml_result_payload", signature = (template, profile = "1.1"))]
fn render_toml_result_payload_py(
    py: Python<'_>,
    template: &Bound<'_, PyAny>,
    profile: &str,
) -> PyResult<(String, Py<PyAny>)> {
    render_toml_result_payload_for_profile(py, template, parse_toml_profile(profile)?)
}

fn render_yaml_for_profile(
    py: Python<'_>,
    template: &Bound<'_, PyAny>,
    profile: YamlProfile,
) -> PyResult<Py<PyAny>> {
    let template = extract_template(py, template, "render_yaml/render_yaml_text")?;
    let node = parse_yaml_template(&template, profile)?;
    let rendered =
        tstring_pyo3_bindings::yaml::render_document_data(py, &template, profile, node.as_ref())
            .map_err(backend_error_to_py)?;
    let mut normalized = tstring_yaml::normalize_documents_with_profile(&rendered, profile)
        .map_err(backend_error_to_py)?;
    tstring_yaml::align_normalized_stream_with_ast(node.as_ref(), &mut normalized);
    normalized_stream_to_python(py, &normalized)
}

fn render_yaml(py: Python<'_>, template: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    render_yaml_for_profile(py, template, YamlProfile::default())
}

#[pyfunction(name = "render_yaml", signature = (template, profile = "1.2.2"))]
fn render_yaml_py(
    py: Python<'_>,
    template: &Bound<'_, PyAny>,
    profile: &str,
) -> PyResult<Py<PyAny>> {
    render_yaml_for_profile(py, template, parse_yaml_profile(profile)?)
}

fn render_yaml_text_for_profile(
    py: Python<'_>,
    template: &Bound<'_, PyAny>,
    profile: YamlProfile,
) -> PyResult<String> {
    let template = extract_template(py, template, "render_yaml/render_yaml_text")?;
    let node = parse_yaml_template(&template, profile)?;
    tstring_pyo3_bindings::yaml::render_document_text(py, &template, profile, node.as_ref())
        .map_err(backend_error_to_py)
}

fn render_yaml_text(py: Python<'_>, template: &Bound<'_, PyAny>) -> PyResult<String> {
    render_yaml_text_for_profile(py, template, YamlProfile::default())
}

#[pyfunction(name = "render_yaml_text", signature = (template, profile = "1.2.2"))]
fn render_yaml_text_py(
    py: Python<'_>,
    template: &Bound<'_, PyAny>,
    profile: &str,
) -> PyResult<String> {
    render_yaml_text_for_profile(py, template, parse_yaml_profile(profile)?)
}

fn render_yaml_result_payload_for_profile(
    py: Python<'_>,
    template: &Bound<'_, PyAny>,
    profile: YamlProfile,
) -> PyResult<(String, Py<PyAny>)> {
    let template = extract_template(py, template, "render_result")?;
    let node = parse_yaml_template(&template, profile)?;
    let rendered =
        tstring_pyo3_bindings::yaml::render_document(py, &template, profile, node.as_ref())
            .map_err(backend_error_to_py)?;
    let mut normalized =
        tstring_yaml::normalize_documents_with_profile(&rendered.documents, profile)
            .map_err(backend_error_to_py)?;
    tstring_yaml::align_normalized_stream_with_ast(node.as_ref(), &mut normalized);
    let data = normalized_stream_to_python(py, &normalized)?;
    Ok((rendered.text, data))
}

fn _render_yaml_result_payload(
    py: Python<'_>,
    template: &Bound<'_, PyAny>,
) -> PyResult<(String, Py<PyAny>)> {
    render_yaml_result_payload_for_profile(py, template, YamlProfile::default())
}

#[pyfunction(name = "_render_yaml_result_payload", signature = (template, profile = "1.2.2"))]
fn render_yaml_result_payload_py(
    py: Python<'_>,
    template: &Bound<'_, PyAny>,
    profile: &str,
) -> PyResult<(String, Py<PyAny>)> {
    render_yaml_result_payload_for_profile(py, template, parse_yaml_profile(profile)?)
}

fn normalized_stream_to_python(py: Python<'_>, stream: &NormalizedStream) -> PyResult<Py<PyAny>> {
    match stream.documents.as_slice() {
        [] => Ok(py.None()),
        [document] => normalized_document_to_python(py, document),
        documents => {
            let list = PyList::empty(py);
            for document in documents {
                list.append(normalized_document_to_python(py, document)?)?;
            }
            Ok(list.into_any().unbind())
        }
    }
}

fn normalized_document_to_python(
    py: Python<'_>,
    document: &NormalizedDocument,
) -> PyResult<Py<PyAny>> {
    match document {
        NormalizedDocument::Empty => Ok(py.None()),
        NormalizedDocument::Value(value) => normalized_value_to_python(py, value),
    }
}

fn normalized_value_to_python(py: Python<'_>, value: &NormalizedValue) -> PyResult<Py<PyAny>> {
    match value {
        NormalizedValue::Null => Ok(py.None()),
        NormalizedValue::Bool(value) => value.into_py_any(py),
        NormalizedValue::Integer(value) => PyModule::import(py, "builtins")?
            .getattr("int")?
            .call1((value.to_string(),))?
            .into_py_any(py),
        NormalizedValue::Float(value) => normalized_float_to_python(py, value),
        NormalizedValue::String(value) => Ok(PyString::new(py, value).into_any().unbind()),
        NormalizedValue::Temporal(value) => normalized_temporal_to_python(py, value),
        NormalizedValue::Sequence(values) => {
            let list = PyList::empty(py);
            for value in values {
                list.append(normalized_value_to_python(py, value)?)?;
            }
            Ok(list.into_any().unbind())
        }
        NormalizedValue::Mapping(values) => {
            let dict = PyDict::new(py);
            for entry in values {
                dict.set_item(
                    normalized_key_to_python(py, &entry.key)?,
                    normalized_value_to_python(py, &entry.value)?,
                )?;
            }
            Ok(dict.into_any().unbind())
        }
        NormalizedValue::Set(values) => {
            let elements = PyList::empty(py);
            for value in values {
                elements.append(normalized_key_to_python(py, value)?)?;
            }
            PyModule::import(py, "builtins")?
                .getattr("set")?
                .call1((elements,))?
                .into_py_any(py)
        }
    }
}

fn normalized_key_to_python(py: Python<'_>, key: &NormalizedKey) -> PyResult<Py<PyAny>> {
    match key {
        NormalizedKey::Null => Ok(py.None()),
        NormalizedKey::Bool(value) => value.into_py_any(py),
        NormalizedKey::Integer(value) => PyModule::import(py, "builtins")?
            .getattr("int")?
            .call1((value.to_string(),))?
            .into_py_any(py),
        NormalizedKey::Float(value) => normalized_float_to_python(py, value),
        NormalizedKey::String(value) => Ok(PyString::new(py, value).into_any().unbind()),
        NormalizedKey::Temporal(value) => normalized_temporal_to_python(py, value),
        NormalizedKey::Sequence(values) => {
            let mut items = Vec::with_capacity(values.len());
            for value in values {
                items.push(normalized_key_to_python(py, value)?);
            }
            Ok(PyTuple::new(py, items)?.into_any().unbind())
        }
        NormalizedKey::Mapping(values) => {
            let pairs = PyList::empty(py);
            for entry in values {
                let pair = PyTuple::new(
                    py,
                    [
                        normalized_key_to_python(py, &entry.key)?,
                        normalized_key_to_python(py, &entry.value)?,
                    ],
                )?;
                pairs.append(pair)?;
            }
            PyModule::import(py, "builtins")?
                .getattr("frozenset")?
                .call1((pairs,))?
                .into_py_any(py)
        }
    }
}

fn normalized_float_to_python(py: Python<'_>, value: &NormalizedFloat) -> PyResult<Py<PyAny>> {
    match value {
        NormalizedFloat::Finite(value) => value.into_py_any(py),
        NormalizedFloat::PosInf => f64::INFINITY.into_py_any(py),
        NormalizedFloat::NegInf => f64::NEG_INFINITY.into_py_any(py),
        NormalizedFloat::NaN => f64::NAN.into_py_any(py),
    }
}

fn normalized_temporal_to_python(
    py: Python<'_>,
    value: &NormalizedTemporal,
) -> PyResult<Py<PyAny>> {
    let datetime = PyModule::import(py, "datetime")?;
    match value {
        NormalizedTemporal::OffsetDateTime(value) => datetime
            .getattr("datetime")?
            .call1((
                value.date.year,
                value.date.month,
                value.date.day,
                value.time.hour,
                value.time.minute,
                value.time.second,
                value.time.nanosecond / 1_000,
            ))?
            .call_method(
                "replace",
                (),
                Some(&{
                    let kwargs = PyDict::new(py);
                    let offset = normalized_offset_to_python(py, value.offset_minutes)?;
                    kwargs.set_item("tzinfo", offset)?;
                    kwargs
                }),
            )
            .map(|value| value.unbind()),
        NormalizedTemporal::LocalDateTime(value) => datetime
            .getattr("datetime")?
            .call1((
                value.date.year,
                value.date.month,
                value.date.day,
                value.time.hour,
                value.time.minute,
                value.time.second,
                value.time.nanosecond / 1_000,
            ))
            .map(|value| value.unbind()),
        NormalizedTemporal::LocalDate(value) => datetime
            .getattr("date")?
            .call1((value.year, value.month, value.day))
            .map(|value| value.unbind()),
        NormalizedTemporal::LocalTime(value) => datetime
            .getattr("time")?
            .call1((
                value.hour,
                value.minute,
                value.second,
                value.nanosecond / 1_000,
            ))
            .map(|value| value.unbind()),
    }
}

fn normalized_offset_to_python(py: Python<'_>, offset_minutes: i16) -> PyResult<Py<PyAny>> {
    let datetime = PyModule::import(py, "datetime")?;
    if offset_minutes == 0 {
        return datetime.getattr("UTC")?.into_py_any(py);
    }
    let total_seconds = i32::from(offset_minutes) * 60;
    let kwargs = PyDict::new(py);
    kwargs.set_item("seconds", total_seconds)?;
    let delta = datetime.getattr("timedelta")?.call((), Some(&kwargs))?;
    datetime
        .getattr("timezone")?
        .call1((delta,))
        .map(|value| value.unbind())
}

#[pymodule]
fn tstring_bindings(py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("__version__", "0.1.0")?;
    module.add("__contract_version__", CONTRACT_VERSION)?;
    module.add("__contract_symbols__", PyTuple::new(py, CONTRACT_SYMBOLS)?)?;
    module.add("TemplateError", py.get_type::<TemplateError>())?;
    module.add("TemplateParseError", py.get_type::<TemplateParseError>())?;
    module.add(
        "TemplateSemanticError",
        py.get_type::<TemplateSemanticError>(),
    )?;
    module.add(
        "UnrepresentableValueError",
        py.get_type::<UnrepresentableValueError>(),
    )?;
    module.add_function(wrap_pyfunction!(render_json_py, module)?)?;
    module.add_function(wrap_pyfunction!(render_json_text_py, module)?)?;
    module.add_function(wrap_pyfunction!(render_json_result_payload_py, module)?)?;
    module.add_function(wrap_pyfunction!(render_toml_py, module)?)?;
    module.add_function(wrap_pyfunction!(render_toml_text_py, module)?)?;
    module.add_function(wrap_pyfunction!(render_toml_result_payload_py, module)?)?;
    module.add_function(wrap_pyfunction!(render_yaml_py, module)?)?;
    module.add_function(wrap_pyfunction!(render_yaml_text_py, module)?)?;
    module.add_function(wrap_pyfunction!(render_yaml_result_payload_py, module)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        _render_json_result_payload, _render_toml_result_payload, _render_yaml_result_payload,
        CacheKey, NormalizedDocument, NormalizedKey, NormalizedValue, ParseCache, TemplateError,
        TemplateParseError, TemplateSemanticError, UnrepresentableValueError, YamlProfile,
        extract_template, parse_yaml_template, render_json, render_json_text, render_toml,
        render_toml_text, render_toml_text_for_profile, render_yaml, render_yaml_text,
        toml_parse_cache, tstring_bindings as init_bindings_module,
    };
    use pyo3::PyTypeInfo;
    use pyo3::exceptions::{PyTypeError, PyValueError};
    use pyo3::prelude::*;
    use pyo3::types::PyModule;
    use std::ffi::CString;
    use tstring_toml::TomlProfile;

    fn cache_key(parts: &[&str]) -> CacheKey {
        (
            "default".to_owned(),
            parts.iter().map(|part| (*part).to_owned()).collect(),
        )
    }
    #[test]
    fn parse_cache_evicts_oldest_entry_when_capacity_is_reached() {
        let cache = ParseCache::new(2);
        let first = cache
            .get_or_try_insert_with(&cache_key(&["a"]), || Ok::<_, ()>(1))
            .unwrap();
        let second = cache
            .get_or_try_insert_with(&cache_key(&["b"]), || Ok::<_, ()>(2))
            .unwrap();
        assert_eq!((*first, *second), (1, 2));

        let cached_first = cache.get(&cache_key(&["a"])).unwrap();
        assert_eq!(*cached_first, 1);

        let third = cache
            .get_or_try_insert_with(&cache_key(&["c"]), || Ok::<_, ()>(3))
            .unwrap();
        assert_eq!(*third, 3);
        assert!(cache.get(&cache_key(&["b"])).is_none());
        assert!(cache.get(&cache_key(&["a"])).is_some());
        assert!(cache.get(&cache_key(&["c"])).is_some());
    }

    #[test]
    fn parse_cache_only_stores_successful_parses() {
        let cache = ParseCache::new(2);
        let err = cache
            .get_or_try_insert_with(&cache_key(&["bad"]), || Err::<usize, _>("boom"))
            .expect_err("expected cache builder error");
        assert_eq!(err, "boom");
        assert_eq!(cache.len(), 0);

        let value = cache
            .get_or_try_insert_with(&cache_key(&["good"]), || Ok::<_, &str>(7usize))
            .unwrap();
        assert_eq!(*value, 7);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn public_toml_pyfunction_defaults_to_profile_1_1_and_separates_cache_entries() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ntemplate=Template('value = 1')\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_profile_defaults.py"),
                pyo3::ffi::c_str!("test_bindings_toml_profile_defaults"),
            )
            .unwrap();
            let template = module.getattr("template").unwrap();
            let bindings = PyModule::new(py, "tstring_bindings").unwrap();
            init_bindings_module(py, &bindings).unwrap();

            let before = toml_parse_cache().len();
            let default_text = bindings
                .getattr("render_toml_text")
                .unwrap()
                .call1((&template,))
                .unwrap()
                .extract::<String>()
                .unwrap();
            let explicit_text = bindings
                .getattr("render_toml_text")
                .unwrap()
                .call1((&template, "1.1"))
                .unwrap()
                .extract::<String>()
                .unwrap();
            let v1_0_text = bindings
                .getattr("render_toml_text")
                .unwrap()
                .call1((&template, "1.0"))
                .unwrap()
                .extract::<String>()
                .unwrap();

            assert_eq!(default_text, explicit_text);
            assert_eq!(v1_0_text, "value = 1");
            assert_eq!(toml_parse_cache().len(), before + 2);
        });
    }

    #[test]
    fn public_pyfunctions_reject_invalid_profiles_defensively() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "json_template=t'{1}'\ntoml_template=t'value = 1'\nyaml_template=t'value: 1'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_invalid_profiles.py"),
                pyo3::ffi::c_str!("test_bindings_invalid_profiles"),
            )
            .unwrap();
            let bindings = PyModule::new(py, "tstring_bindings").unwrap();
            init_bindings_module(py, &bindings).unwrap();

            for (name, profile) in [
                ("render_json_text", "draft"),
                ("render_toml_text", "2.0"),
                ("render_yaml_text", "1.1"),
            ] {
                let template = match name {
                    "render_json_text" => module.getattr("json_template").unwrap(),
                    "render_toml_text" => module.getattr("toml_template").unwrap(),
                    _ => module.getattr("yaml_template").unwrap(),
                };
                let err = bindings
                    .getattr(name)
                    .unwrap()
                    .call1((template, profile))
                    .expect_err("expected invalid profile error");
                assert!(err.is_instance_of::<PyValueError>(py));
            }
        });
    }

    #[test]
    fn bindings_reuse_cached_parse_across_runtime_values() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "json_value_one=1\njson_value_two=2\ntoml_value_one=1\ntoml_value_two=2\nyaml_value_one=1\nyaml_value_two=2\njson_one=t'{{\"value\": {json_value_one}}}'\njson_two=t'{{\"value\": {json_value_two}}}'\ntoml_one=t'value = {toml_value_one}'\ntoml_two=t'value = {toml_value_two}'\nyaml_one=t'value: {yaml_value_one}'\nyaml_two=t'value: {yaml_value_two}'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_parse_cache_values.py"),
                pyo3::ffi::c_str!("test_bindings_parse_cache_values"),
            )
            .unwrap();

            assert_eq!(
                render_json_text(py, &module.getattr("json_one").unwrap()).unwrap(),
                "{\"value\": 1}"
            );
            assert_eq!(
                render_json_text(py, &module.getattr("json_two").unwrap()).unwrap(),
                "{\"value\": 2}"
            );

            assert_eq!(
                render_toml_text(py, &module.getattr("toml_one").unwrap()).unwrap(),
                "value = 1"
            );
            assert_eq!(
                render_toml_text(py, &module.getattr("toml_two").unwrap()).unwrap(),
                "value = 2"
            );

            assert_eq!(
                render_yaml_text(py, &module.getattr("yaml_one").unwrap()).unwrap(),
                "value: 1"
            );
            assert_eq!(
                render_yaml_text(py, &module.getattr("yaml_two").unwrap()).unwrap(),
                "value: 2"
            );
        });
    }

    #[test]
    fn bindings_keep_current_expression_labels_when_cached_parse_is_reused() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "json_bad_a={1}\njson_bad_b={2}\ntoml_bad_a=1\ntoml_bad_b=2\nyaml_bad_a='bad tag'\nyaml_bad_b='other tag'\njson_one=t'{{\"value\": {json_bad_a}}}'\njson_two=t'{{\"value\": {json_bad_b}}}'\ntoml_one=t'{toml_bad_a} = 1'\ntoml_two=t'{toml_bad_b} = 1'\nyaml_one=t'!{yaml_bad_a} value'\nyaml_two=t'!{yaml_bad_b} value'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_parse_cache_exprs.py"),
                pyo3::ffi::c_str!("test_bindings_parse_cache_exprs"),
            )
            .unwrap();

            let err = render_json_text(py, &module.getattr("json_one").unwrap())
                .expect_err("expected first JSON render error");
            assert!(err.to_string().contains("json_bad_a"));
            let err = render_json_text(py, &module.getattr("json_two").unwrap())
                .expect_err("expected second JSON render error");
            assert!(err.to_string().contains("json_bad_b"));

            let err = render_toml_text(py, &module.getattr("toml_one").unwrap())
                .expect_err("expected first TOML render error");
            assert!(err.to_string().contains("toml_bad_a"));
            let err = render_toml_text(py, &module.getattr("toml_two").unwrap())
                .expect_err("expected second TOML render error");
            assert!(err.to_string().contains("toml_bad_b"));

            let err = render_yaml_text(py, &module.getattr("yaml_one").unwrap())
                .expect_err("expected first YAML render error");
            assert!(err.to_string().contains("yaml_bad_a"));
            let err = render_yaml_text(py, &module.getattr("yaml_two").unwrap())
                .expect_err("expected second YAML render error");
            assert!(err.to_string().contains("yaml_bad_b"));
        });
    }

    #[test]
    fn bindings_use_single_runtime_path_across_bindings() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nname='Alice'\nnumber=1\nrows=[{'name': 'one'}, {'name': 'two'}]\njson_template=t'{rows}'\njson_invalid=Template('{name: 1}')\ntoml_template=t'name = {name}'\ntoml_invalid=Template('name = ')\nyaml_template=t'name: {name}'\nyaml_invalid=Template('value: [1, 2')\nyaml_unknown_anchor=Template('value: *not_alias\\n')\nyaml_tagged=t'value: !!str {number}'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_validation.py"),
                pyo3::ffi::c_str!("test_bindings_validation"),
            )
            .unwrap();

            let json_template = module.getattr("json_template").unwrap();
            let json_text = render_json_text(py, &json_template).unwrap();
            assert_eq!(json_text, "[{\"name\": \"one\"}, {\"name\": \"two\"}]");

            let toml_template = module.getattr("toml_template").unwrap();
            let toml_text = render_toml_text(py, &toml_template).unwrap();
            assert_eq!(toml_text, "name = \"Alice\"");

            let yaml_template = module.getattr("yaml_template").unwrap();
            let yaml_text = render_yaml_text(py, &yaml_template).unwrap();
            assert_eq!(yaml_text, "name: \"Alice\"");
            let yaml_tagged = render_yaml(py, &module.getattr("yaml_tagged").unwrap()).unwrap();
            let tagged_value = yaml_tagged.bind(py).get_item("value").unwrap();
            assert_eq!(tagged_value.extract::<String>().unwrap(), "1");

            let json_invalid = module.getattr("json_invalid").unwrap();
            let err = render_json_text(py, &json_invalid).expect_err("expected JSON parse error");
            assert!(err.is_instance_of::<TemplateParseError>(py));

            let toml_invalid = module.getattr("toml_invalid").unwrap();
            let err = render_toml_text(py, &toml_invalid).expect_err("expected TOML parse error");
            assert!(err.is_instance_of::<TemplateParseError>(py));

            let yaml_invalid = module.getattr("yaml_invalid").unwrap();
            let err = render_yaml_text(py, &yaml_invalid).expect_err("expected YAML parse error");
            assert!(err.is_instance_of::<TemplateParseError>(py));

            let yaml_unknown_anchor = module.getattr("yaml_unknown_anchor").unwrap();
            let err = render_yaml(py, &yaml_unknown_anchor)
                .expect_err("expected YAML unknown-anchor data error");
            assert!(err.is_instance_of::<TemplateSemanticError>(py));
        });
    }

    #[test]
    fn yaml_materializes_core_string_tags_from_render_model() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("number=1\nyaml_tagged=t'value: !!str {number}'\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_false_mode.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_false_mode"),
            )
            .unwrap();

            let template =
                extract_template(py, &module.getattr("yaml_tagged").unwrap(), "render_yaml")
                    .unwrap();
            let node = parse_yaml_template(&template, YamlProfile::default()).unwrap();
            let rendered = tstring_pyo3_bindings::yaml::render_document(
                py,
                &template,
                YamlProfile::default(),
                node.as_ref(),
            )
            .unwrap();
            let normalized = tstring_yaml::normalize_documents_with_profile(
                &rendered.documents,
                YamlProfile::default(),
            )
            .unwrap();
            let document = normalized
                .documents
                .first()
                .expect("expected a normalized YAML document");
            let NormalizedDocument::Value(NormalizedValue::Mapping(entries)) = document else {
                panic!("expected a normalized YAML mapping document, got {document:?}");
            };
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].key, NormalizedKey::String("value".to_owned()));
            assert_eq!(
                entries[0].value,
                NormalizedValue::String("1".to_owned()),
                "rendered_documents={:?}",
                rendered.documents
            );
        });
    }

    #[test]
    fn yaml_preserves_quoted_scalar_style_under_custom_tags() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "custom=t'value: !custom \"01\"'\nverbatim=t'value: !<tag:example.com,2020:custom> \"01\"'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_custom_tag_false_mode.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_custom_tag_false_mode"),
            )
            .unwrap();

            let custom = render_yaml(py, &module.getattr("custom").unwrap()).unwrap();
            let custom_value = custom.bind(py).get_item("value").unwrap();
            assert_eq!(custom_value.extract::<String>().unwrap(), "01");

            let verbatim = render_yaml(py, &module.getattr("verbatim").unwrap()).unwrap();
            let verbatim_value = verbatim.bind(py).get_item("value").unwrap();
            assert_eq!(verbatim_value.extract::<String>().unwrap(), "01");
        });
    }

    #[test]
    fn json_negative_zero_matches_python_json_semantics_via_bindings() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nrender_jsonop=t'-0'\njson_nested=Template('{\"value\": -0}')\njson_exp=t'-0e0'\njson_float=t'-0.0'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_numbers.py"),
                pyo3::ffi::c_str!("test_bindings_json_numbers"),
            )
            .unwrap();

            let top = render_json(py, &module.getattr("render_jsonop").unwrap()).unwrap();
            let nested = render_json(py, &module.getattr("json_nested").unwrap()).unwrap();
            let exp = render_json(py, &module.getattr("json_exp").unwrap()).unwrap();
            let float_value = render_json(py, &module.getattr("json_float").unwrap()).unwrap();

            let top_repr = top.bind(py).repr().unwrap().extract::<String>().unwrap();
            let top_type = top
                .bind(py)
                .get_type()
                .name()
                .unwrap()
                .extract::<String>()
                .unwrap();
            assert_eq!((top_repr, top_type), ("0".to_owned(), "int".to_owned()));

            let nested_value = nested.bind(py).get_item("value").unwrap();
            let nested_repr = nested_value.repr().unwrap().extract::<String>().unwrap();
            let nested_type = nested_value
                .get_type()
                .name()
                .unwrap()
                .extract::<String>()
                .unwrap();
            assert_eq!(
                (nested_repr, nested_type),
                ("0".to_owned(), "int".to_owned())
            );

            let exp_repr = exp.bind(py).repr().unwrap().extract::<String>().unwrap();
            let exp_type = exp
                .bind(py)
                .get_type()
                .name()
                .unwrap()
                .extract::<String>()
                .unwrap();
            assert_eq!(
                (exp_repr, exp_type),
                ("-0.0".to_owned(), "float".to_owned())
            );

            let float_repr = float_value
                .bind(py)
                .repr()
                .unwrap()
                .extract::<String>()
                .unwrap();
            let float_type = float_value
                .bind(py)
                .get_type()
                .name()
                .unwrap()
                .extract::<String>()
                .unwrap();
            assert_eq!(
                (float_repr, float_type),
                ("-0.0".to_owned(), "float".to_owned())
            );
        });
    }

    #[test]
    fn render_tomlemporal_values_round_trip_via_bindings() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from datetime import date, time\nday=date(2024, 1, 2)\nmoment=time(4, 5, 6)\ntemplate=t'day = {day}\\nmoment = {moment}'\nexpected={'day': day, 'moment': moment}\n"
                ),
                pyo3::ffi::c_str!("test_bindings_render_tomlemporal.py"),
                pyo3::ffi::c_str!("test_bindings_render_tomlemporal"),
            )
            .unwrap();

            let actual = render_toml(py, &module.getattr("template").unwrap()).unwrap();
            let expected = module.getattr("expected").unwrap();
            assert!(actual.bind(py).eq(expected).unwrap());
        });
    }

    #[test]
    fn yaml_complex_mapping_keys_normalize_to_hashable_plain_data_via_bindings() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "left='Alice'\nright='Bob'\nexpected={frozenset({('name', ('Alice', 'Bob'))}): 1, ('Alice', 'Bob'): 2}\ntemplate=t'{{ {{name: [{left}, {right}]}}: 1, [{left}, {right}]: 2 }}'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_keys.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_keys"),
            )
            .unwrap();

            let actual = render_yaml(py, &module.getattr("template").unwrap()).unwrap();
            let expected = module.getattr("expected").unwrap();
            assert!(actual.bind(py).eq(expected).unwrap());
        });
    }

    #[test]
    fn bindings_require_template_objects() {
        Python::with_gil(|py| {
            let not_a_template = "not-a-template".into_pyobject(py).unwrap();

            let err = render_json(py, &not_a_template).expect_err("expected JSON type error");
            assert!(err.is_instance_of::<PyTypeError>(py));
            assert!(
                err.to_string()
                    .contains("require a PEP 750 Template object")
            );

            let err = render_toml(py, &not_a_template).expect_err("expected TOML type error");
            assert!(err.is_instance_of::<PyTypeError>(py));
            assert!(
                err.to_string()
                    .contains("require a PEP 750 Template object")
            );

            let err = render_yaml(py, &not_a_template).expect_err("expected YAML type error");
            assert!(err.is_instance_of::<PyTypeError>(py));
            assert!(
                err.to_string()
                    .contains("require a PEP 750 Template object")
            );
        });
    }

    #[test]
    fn bindings_map_backend_error_kinds_to_python_exceptions() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from datetime import UTC, time\nfrom string.templatelib import Template\nbad_key=3\nbad_time=time(1, 2, 3, tzinfo=UTC)\njson_unrepresentable=t'{{{bad_key}: 1}}'\ntoml_duplicate=Template('[a]\\nvalue = 1\\n[a]\\nname = \"x\"\\n')\ntoml_unrepresentable=t'when = {bad_time}'\nyaml_unknown_anchor=Template('value: *not_alias\\n')\nyaml_unrepresentable=t'value: {float(\"inf\")}'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_error_mapping.py"),
                pyo3::ffi::c_str!("test_bindings_error_mapping"),
            )
            .unwrap();

            let json_unrepresentable = module.getattr("json_unrepresentable").unwrap();
            let err = render_json_text(py, &json_unrepresentable)
                .expect_err("expected JSON unrepresentable error");
            assert!(err.is_instance_of::<UnrepresentableValueError>(py));

            let toml_duplicate = module.getattr("toml_duplicate").unwrap();
            let err = render_toml(py, &toml_duplicate).expect_err("expected TOML validation error");
            assert!(err.is_instance_of::<TemplateSemanticError>(py));

            let toml_unrepresentable = module.getattr("toml_unrepresentable").unwrap();
            let err = render_toml_text(py, &toml_unrepresentable)
                .expect_err("expected TOML unrepresentable error");
            assert!(err.is_instance_of::<UnrepresentableValueError>(py));

            let yaml_unknown_anchor = module.getattr("yaml_unknown_anchor").unwrap();
            let err =
                render_yaml_text(py, &yaml_unknown_anchor).expect_err("expected YAML final error");
            assert!(err.is_instance_of::<TemplateSemanticError>(py));

            let yaml_unrepresentable = module.getattr("yaml_unrepresentable").unwrap();
            let err = render_yaml_text(py, &yaml_unrepresentable)
                .expect_err("expected YAML unrepresentable error");
            assert!(err.is_instance_of::<UnrepresentableValueError>(py));
        });
    }

    #[test]
    fn bindings_expose_exception_hierarchy_and_base_instantiation() {
        Python::with_gil(|py| {
            let builtins = PyModule::import(py, "builtins").unwrap();
            let issubclass = builtins.getattr("issubclass").unwrap();

            let parse_is_template: bool = issubclass
                .call1((
                    TemplateParseError::type_object(py),
                    TemplateError::type_object(py),
                ))
                .unwrap()
                .extract()
                .unwrap();
            assert!(parse_is_template);

            let final_is_template: bool = issubclass
                .call1((
                    TemplateSemanticError::type_object(py),
                    TemplateError::type_object(py),
                ))
                .unwrap()
                .extract()
                .unwrap();
            assert!(final_is_template);

            let unrepr_is_semantic: bool = issubclass
                .call1((
                    UnrepresentableValueError::type_object(py),
                    TemplateSemanticError::type_object(py),
                ))
                .unwrap()
                .extract()
                .unwrap();
            assert!(unrepr_is_semantic);

            let base = TemplateError::new_err("message");
            let message = base.value(py).str().unwrap().extract::<String>().unwrap();
            assert_eq!(message, "message");
            assert!(base.is_instance_of::<TemplateError>(py));
        });
    }

    #[test]
    fn bindings_surface_json_fragment_stringification_errors() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "class BadStringValue:\n    def __str__(self):\n        raise RuntimeError('boom')\n\nbad = BadStringValue()\ntemplate=t'{{\"name\": \"prefix-{bad}\"}}'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_fragment_error.py"),
                pyo3::ffi::c_str!("test_bindings_json_fragment_error"),
            )
            .unwrap();

            let template = module.getattr("template").unwrap();
            let err = render_json_text(py, &template).expect_err("expected JSON fragment error");
            assert!(err.is_instance_of::<UnrepresentableValueError>(py));
            assert!(err.to_string().contains("string fragment"));
        });
    }

    #[test]
    fn bindings_surface_toml_parse_and_validation_messages() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nparse_template=Template('value = [1,,2]\\n')\nvalidation_template=Template('[a]\\nvalue = 1\\n[a]\\nname = \"x\"\\n')\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_errors.py"),
                pyo3::ffi::c_str!("test_bindings_toml_errors"),
            )
            .unwrap();

            let parse_template = module.getattr("parse_template").unwrap();
            let err = render_toml(py, &parse_template).expect_err("expected TOML parse-time error");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(err.to_string().contains("Expected a TOML value"));

            let validation_template = module.getattr("validation_template").unwrap();
            let err = render_toml(py, &validation_template)
                .expect_err("expected TOML duplicate-key semantic error");
            assert!(err.is_instance_of::<TemplateSemanticError>(py));
            assert!(err.to_string().to_lowercase().contains("duplicate"));
        });
    }

    #[test]
    fn bindings_surface_yaml_metadata_and_anchor_errors() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "tag='bad tag'\nmetadata_template=t'value: !{tag} ok'\nanchor_template=__import__('string.templatelib').templatelib.Template('value: *not_alias\\n')\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_errors.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_errors"),
            )
            .unwrap();

            let metadata_template = module.getattr("metadata_template").unwrap();
            let err =
                render_yaml_text(py, &metadata_template).expect_err("expected YAML metadata error");
            assert!(err.is_instance_of::<UnrepresentableValueError>(py));
            assert!(err.to_string().contains("metadata"));

            let anchor_template = module.getattr("anchor_template").unwrap();
            let err =
                render_yaml_text(py, &anchor_template).expect_err("expected YAML anchor error");
            assert!(err.is_instance_of::<TemplateSemanticError>(py));
            assert!(err.to_string().contains("unknown anchor"));
        });
    }

    #[test]
    fn bindings_surface_json_unrepresentable_value_messages() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "bad_key=3\nbad_value={1, 2}\nkey_template=t'{{{bad_key}: 1}}'\nvalue_template=t'{{\"items\": {bad_value}}}'\nfloat_template=t'{{\"ratio\": {float(\"inf\")}}}'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_unrepresentable.py"),
                pyo3::ffi::c_str!("test_bindings_json_unrepresentable"),
            )
            .unwrap();

            let key_template = module.getattr("key_template").unwrap();
            let err =
                render_json_text(py, &key_template).expect_err("expected JSON object-key error");
            assert!(err.is_instance_of::<UnrepresentableValueError>(py));
            assert!(err.to_string().contains("object key"));

            let value_template = module.getattr("value_template").unwrap();
            let err = render_json_text(py, &value_template)
                .expect_err("expected JSON set conversion error");
            assert!(err.is_instance_of::<UnrepresentableValueError>(py));
            assert!(err.to_string().contains("set"));

            let float_template = module.getattr("float_template").unwrap();
            let err = render_json_text(py, &float_template)
                .expect_err("expected JSON non-finite float error");
            assert!(err.is_instance_of::<UnrepresentableValueError>(py));
            assert!(err.to_string().contains("non-finite float"));
        });
    }

    #[test]
    fn bindings_surface_toml_unrepresentable_value_messages() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from datetime import UTC, time\nclass BadStringValue:\n    def __str__(self):\n        raise RuntimeError('boom')\n\nbad_key=3\nbad_time=time(1, 2, 3, tzinfo=UTC)\nbad_fragment=BadStringValue()\nkey_template=t'{bad_key} = 1'\ntime_template=t'when = {bad_time}'\nfragment_template=t'title = \"hi-{bad_fragment}\"'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_unrepresentable.py"),
                pyo3::ffi::c_str!("test_bindings_toml_unrepresentable"),
            )
            .unwrap();

            let key_template = module.getattr("key_template").unwrap();
            let err = render_toml_text(py, &key_template).expect_err("expected TOML key error");
            assert!(err.is_instance_of::<UnrepresentableValueError>(py));
            assert!(err.to_string().contains("TOML key"));

            let time_template = module.getattr("time_template").unwrap();
            let err =
                render_toml_text(py, &time_template).expect_err("expected TOML timezone error");
            assert!(err.is_instance_of::<UnrepresentableValueError>(py));
            assert!(err.to_string().contains("timezone"));

            let fragment_template = module.getattr("fragment_template").unwrap();
            let err = render_toml_text(py, &fragment_template)
                .expect_err("expected TOML fragment conversion error");
            assert!(err.is_instance_of::<UnrepresentableValueError>(py));
            assert!(err.to_string().contains("string fragment"));
        });
    }

    #[test]
    fn bindings_surface_yaml_parse_and_non_finite_float_messages() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nparse_template=Template('[1,,2]\\n')\nfloat_template=t'value: {float(\"inf\")}'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_parse_and_float.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_parse_and_float"),
            )
            .unwrap();

            let parse_template = module.getattr("parse_template").unwrap();
            let err = render_yaml_text(py, &parse_template).expect_err("expected YAML parse error");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(err.to_string().contains("Expected a YAML value"));

            let float_template = module.getattr("float_template").unwrap();
            let err = render_yaml_text(py, &float_template)
                .expect_err("expected YAML non-finite float error");
            assert!(err.is_instance_of::<UnrepresentableValueError>(py));
            assert!(err.to_string().contains("non-finite float"));
        });
    }

    #[test]
    fn bindings_surface_json_parse_messages() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nbare_key=t'{{name: 1}}'\nunterminated=Template('{\"name\": \"alice}')\ninvalid_number=Template('1e+')\ninvalid_value=Template('[1,,2]')\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_parse_messages.py"),
                pyo3::ffi::c_str!("test_bindings_json_parse_messages"),
            )
            .unwrap();

            let bare_key = module.getattr("bare_key").unwrap();
            let err = render_json_text(py, &bare_key).expect_err("expected JSON bare-key error");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(err.to_string().contains("JSON object keys"));

            let unterminated = module.getattr("unterminated").unwrap();
            let err = render_json_text(py, &unterminated).expect_err("expected JSON string error");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(err.to_string().contains("Unterminated JSON string"));

            let invalid_number = module.getattr("invalid_number").unwrap();
            let err =
                render_json_text(py, &invalid_number).expect_err("expected JSON number error");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(err.to_string().contains("Invalid JSON number literal"));

            let invalid_value = module.getattr("invalid_value").unwrap();
            let err = render_json_text(py, &invalid_value).expect_err("expected JSON value error");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(err.to_string().contains("Expected a JSON value"));
        });
    }

    #[test]
    fn bindings_surface_toml_parse_messages() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nnewline_string=t'value = \"a\\nb\"'\ninvalid_segment=Template('a.\\nb = 1\\n')\ninvalid_literal=Template('value = 1__2\\n')\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_parse_messages.py"),
                pyo3::ffi::c_str!("test_bindings_toml_parse_messages"),
            )
            .unwrap();

            let newline_string = module.getattr("newline_string").unwrap();
            let err = render_toml_text(py, &newline_string)
                .expect_err("expected TOML single-line string error");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(
                err.to_string()
                    .contains("single-line basic strings cannot contain newlines")
            );

            let invalid_segment = module.getattr("invalid_segment").unwrap();
            let err = render_toml(py, &invalid_segment)
                .expect_err("expected TOML key-segment parse error");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(err.to_string().contains("Expected a TOML key segment"));

            let invalid_literal = module.getattr("invalid_literal").unwrap();
            let err =
                render_toml(py, &invalid_literal).expect_err("expected TOML literal parse error");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(err.to_string().contains("Invalid TOML literal"));
        });
    }

    #[test]
    fn bindings_surface_yaml_parse_messages() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nmissing_colon=Template('value: {a b}\\n')\ntabbed=Template('a:\\t1\\n')\ntrailing_alias=Template('value: *not alias\\n')\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_parse_messages.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_parse_messages"),
            )
            .unwrap();

            let missing_colon = module.getattr("missing_colon").unwrap();
            let err = render_yaml_text(py, &missing_colon)
                .expect_err("expected YAML missing-colon parse error");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(err.to_string().contains("Expected ':' in YAML template"));

            let tabbed = module.getattr("tabbed").unwrap();
            let err = render_yaml_text(py, &tabbed).expect_err("expected YAML tab parse error");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(err.to_string().contains("Tabs are not allowed"));

            let trailing_alias = module.getattr("trailing_alias").unwrap();
            let err = render_yaml_text(py, &trailing_alias)
                .expect_err("expected YAML trailing-content parse error");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(err.to_string().contains("Unexpected trailing YAML content"));
        });
    }

    #[test]
    fn bindings_surface_json_escape_and_unicode_parse_messages() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ninvalid_escape=Template('\"\\\\x41\"')\nshort_unicode=Template('\"\\\\u12\"')\ninvalid_unicode=Template('\"\\\\uZZZZ\"')\ncontrol_char=Template('\"a\\nb\"')\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_escape_messages.py"),
                pyo3::ffi::c_str!("test_bindings_json_escape_messages"),
            )
            .unwrap();

            let invalid_escape = module.getattr("invalid_escape").unwrap();
            let err = render_json_text(py, &invalid_escape)
                .expect_err("expected JSON escape-sequence parse error");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(err.to_string().contains("Invalid JSON escape sequence"));

            let short_unicode = module.getattr("short_unicode").unwrap();
            let err = render_json_text(py, &short_unicode)
                .expect_err("expected short JSON unicode escape parse error");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(
                err.to_string()
                    .contains("Unexpected end of JSON escape sequence")
            );

            let invalid_unicode = module.getattr("invalid_unicode").unwrap();
            let err = render_json_text(py, &invalid_unicode)
                .expect_err("expected invalid JSON unicode escape parse error");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(err.to_string().contains("Invalid JSON unicode escape"));

            let control_char = module.getattr("control_char").unwrap();
            let err = render_json_text(py, &control_char)
                .expect_err("expected JSON control-character parse error");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(
                err.to_string()
                    .contains("Control characters are not allowed")
            );
        });
    }

    #[test]
    fn bindings_surface_toml_null_message() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("template=t'name = {None}'\n"),
                pyo3::ffi::c_str!("test_bindings_toml_null.py"),
                pyo3::ffi::c_str!("test_bindings_toml_null"),
            )
            .unwrap();

            let template = module.getattr("template").unwrap();
            let err =
                render_toml_text(py, &template).expect_err("expected TOML null error message");
            assert!(err.is_instance_of::<UnrepresentableValueError>(py));
            assert!(err.to_string().contains("no null value"));
        });
    }

    #[test]
    fn bindings_surface_yaml_fragment_message() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "class BadStringValue:\n    def __str__(self):\n        raise RuntimeError('boom')\n\nbad = BadStringValue()\ntemplate=t'label: \"hi-{bad}\"'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_fragment.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_fragment"),
            )
            .unwrap();

            let template = module.getattr("template").unwrap();
            let err = render_yaml_text(py, &template).expect_err("expected YAML fragment error");
            assert!(err.is_instance_of::<UnrepresentableValueError>(py));
            assert!(err.to_string().contains("fragment"));
        });
    }

    #[test]
    fn bindings_surface_render_jsonop_level_values_and_quoted_key_fragments() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "left='prefix'\nright='suffix'\nrows=[{'name': 'one'}, {'name': 'two'}]\nquoted_key=t'{{\"{left}-{right}\": {1}, \"value\": {True}}}'\npromoted=t'{rows}'\nscalar=t'{1}'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_shapes.py"),
                pyo3::ffi::c_str!("test_bindings_json_shapes"),
            )
            .unwrap();

            let quoted_key = render_json(py, &module.getattr("quoted_key").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'prefix-suffix': 1, 'value': True}\n"),
                pyo3::ffi::c_str!("test_bindings_json_shapes_expected.py"),
                pyo3::ffi::c_str!("test_bindings_json_shapes_expected"),
            )
            .unwrap();
            assert!(
                quoted_key
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let promoted = render_json(py, &module.getattr("promoted").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=[{'name': 'one'}, {'name': 'two'}]\n"),
                pyo3::ffi::c_str!("test_bindings_json_shapes_expected_list.py"),
                pyo3::ffi::c_str!("test_bindings_json_shapes_expected_list"),
            )
            .unwrap();
            assert!(
                promoted
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let scalar_text = render_json_text(py, &module.getattr("scalar").unwrap()).unwrap();
            assert_eq!(scalar_text, "1");
        });
    }

    #[test]
    fn bindings_surface_toml_inline_tables_and_header_progressions() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ninline_table=Template('value = { inner = { deep = { value = 1 } } }\\n')\nheader_progression=Template('[\"a.b\"]\\nvalue = 1\\n\\n[\"a.b\".c]\\nname = \"x\"\\n')\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_shapes.py"),
                pyo3::ffi::c_str!("test_bindings_toml_shapes"),
            )
            .unwrap();

            let inline_table = render_toml(py, &module.getattr("inline_table").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'value': {'inner': {'deep': {'value': 1}}}}\n"),
                pyo3::ffi::c_str!("test_bindings_toml_shapes_expected.py"),
                pyo3::ffi::c_str!("test_bindings_toml_shapes_expected"),
            )
            .unwrap();
            assert!(
                inline_table
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let header_progression =
                render_toml(py, &module.getattr("header_progression").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'a.b': {'value': 1, 'c': {'name': 'x'}}}\n"),
                pyo3::ffi::c_str!("test_bindings_toml_shapes_expected_header.py"),
                pyo3::ffi::c_str!("test_bindings_toml_shapes_expected_header"),
            )
            .unwrap();
            assert!(
                header_progression
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
        });
    }

    #[test]
    fn bindings_surface_yaml_custom_tags_and_core_schema() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "custom_mapping=t'value: !custom 3\\n'\ncustom_sequence=t'value: !custom [1, 2]\\n'\ncore_mapping=t'value_bool: !!bool true\\nvalue_str: !!str true\\nvalue_float: !!float 1\\nvalue_null: !!null null\\n'\nroot_int=t'--- !!int 3\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_shapes.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_shapes"),
            )
            .unwrap();

            let custom_mapping =
                render_yaml(py, &module.getattr("custom_mapping").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'value': 3}\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_shapes_expected.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_shapes_expected"),
            )
            .unwrap();
            assert!(
                custom_mapping
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let custom_sequence =
                render_yaml(py, &module.getattr("custom_sequence").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'value': [1, 2]}\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_shapes_expected_seq.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_shapes_expected_seq"),
            )
            .unwrap();
            assert!(
                custom_sequence
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let core_mapping = render_yaml(py, &module.getattr("core_mapping").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "expected={'value_bool': True, 'value_str': 'true', 'value_float': 1.0, 'value_null': None}\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_shapes_expected_core.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_shapes_expected_core"),
            )
            .unwrap();
            assert!(
                core_mapping
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let root_int = render_yaml(py, &module.getattr("root_int").unwrap()).unwrap();
            assert_eq!(root_int.bind(py).extract::<i64>().unwrap(), 3);
        });
    }

    #[test]
    fn bindings_surface_yaml_custom_tag_scalar_mapping_and_root_sequence() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "scalar_root=t'!custom 3\\n'\nmapping=t'value: !custom 3\\n'\nsequence=t'value: !custom [1, 2]\\n'\ncommented_root_sequence=t'--- # comment\\n!custom [1, 2]\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_custom_tag_round_trip.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_custom_tag_round_trip"),
            )
            .unwrap();

            let scalar_root = render_yaml(py, &module.getattr("scalar_root").unwrap()).unwrap();
            assert_eq!(scalar_root.bind(py).extract::<i64>().unwrap(), 3);

            let mapping = render_yaml(py, &module.getattr("mapping").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'value': 3}\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_custom_tag_round_trip_expected_map.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_custom_tag_round_trip_expected_map"),
            )
            .unwrap();
            assert!(
                mapping
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let sequence = render_yaml(py, &module.getattr("sequence").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'value': [1, 2]}\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_custom_tag_round_trip_expected_seq.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_custom_tag_round_trip_expected_seq"),
            )
            .unwrap();
            assert!(
                sequence
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let commented_root_sequence =
                render_yaml(py, &module.getattr("commented_root_sequence").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=[1, 2]\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_custom_tag_round_trip_expected_root.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_custom_tag_round_trip_expected_root"),
            )
            .unwrap();
            assert!(
                commented_root_sequence
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let scalar_text =
                render_yaml_text(py, &module.getattr("scalar_root").unwrap()).unwrap();
            let mapping_text = render_yaml_text(py, &module.getattr("mapping").unwrap()).unwrap();
            assert_eq!(scalar_text, "!custom 3");
            assert_eq!(mapping_text, "value: !custom 3");
        });
    }

    #[test]
    fn bindings_surface_yaml_explicit_core_tag_mapping_and_root_text_families() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "mapping=t'value_bool: !!bool true\\nvalue_str: !!str true\\nvalue_float: !!float 1\\nvalue_null: !!null null\\n'\nroot_int=t'--- !!int 3\\n'\nroot_str=t'--- !!str true\\n'\nroot_bool=t'--- !!bool true\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_explicit_core_tag_families.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_explicit_core_tag_families"),
            )
            .unwrap();

            let mapping = render_yaml(py, &module.getattr("mapping").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "expected={'value_bool': True, 'value_str': 'true', 'value_float': 1.0, 'value_null': None}\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_explicit_core_tag_families_expected_map.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_explicit_core_tag_families_expected_map"),
            )
            .unwrap();
            assert!(
                mapping
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let root_int = render_yaml(py, &module.getattr("root_int").unwrap()).unwrap();
            let root_str = render_yaml(py, &module.getattr("root_str").unwrap()).unwrap();
            let root_bool = render_yaml(py, &module.getattr("root_bool").unwrap()).unwrap();
            assert_eq!(root_int.bind(py).extract::<i64>().unwrap(), 3);
            assert_eq!(root_str.bind(py).extract::<String>().unwrap(), "true");
            assert!(root_bool.bind(py).extract::<bool>().unwrap());

            let root_int_text = render_yaml_text(py, &module.getattr("root_int").unwrap()).unwrap();
            let root_str_text = render_yaml_text(py, &module.getattr("root_str").unwrap()).unwrap();
            let root_bool_text =
                render_yaml_text(py, &module.getattr("root_bool").unwrap()).unwrap();
            assert_eq!(root_int_text, "---\n!!int 3");
            assert_eq!(root_str_text, "---\n!!str true");
            assert_eq!(root_bool_text, "---\n!!bool true");
        });
    }

    #[test]
    fn bindings_surface_yaml_flow_trailing_comma_explicit_key_and_indent_indicator() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nflow_sequence=t'[1, 2,]\\n'\nflow_mapping=Template('{a: 1,}\\n')\nexplicit_key_sequence_value=t'? a\\n: - 1\\n  - 2\\n'\nindent_indicator=t'value: |1\\n a\\n b\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_indent_families.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_indent_families"),
            )
            .unwrap();

            let flow_sequence = render_yaml(py, &module.getattr("flow_sequence").unwrap()).unwrap();
            let flow_mapping = render_yaml(py, &module.getattr("flow_mapping").unwrap()).unwrap();
            let explicit_key_sequence_value =
                render_yaml(py, &module.getattr("explicit_key_sequence_value").unwrap()).unwrap();
            let indent_indicator =
                render_yaml(py, &module.getattr("indent_indicator").unwrap()).unwrap();
            let indent_indicator_text =
                render_yaml_text(py, &module.getattr("indent_indicator").unwrap()).unwrap();

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=[1, 2]\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_indent_families_expected_seq.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_indent_families_expected_seq"),
            )
            .unwrap();
            assert!(
                flow_sequence
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'a': 1}\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_indent_families_expected_map.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_indent_families_expected_map"),
            )
            .unwrap();
            assert!(
                flow_mapping
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'a': [1, 2]}\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_indent_families_expected_explicit.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_indent_families_expected_explicit"),
            )
            .unwrap();
            assert!(
                explicit_key_sequence_value
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'value': 'a\\nb\\n'}\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_indent_families_expected_indent.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_indent_families_expected_indent"),
            )
            .unwrap();
            assert!(
                indent_indicator
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
            assert_eq!(indent_indicator_text, "value: |1\n a\n b\n");
        });
    }

    #[test]
    fn bindings_surface_json_validation_text_and_data() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("name='Alice'\ntemplate=t'{{\"name\": {name}}}'\n"),
                pyo3::ffi::c_str!("test_bindings_json_validation_text.py"),
                pyo3::ffi::c_str!("test_bindings_json_validation_text"),
            )
            .unwrap();

            let template = module.getattr("template").unwrap();
            let validated = render_json_text(py, &template).unwrap();
            let unvalidated = render_json_text(py, &template).unwrap();
            assert_eq!(validated, "{\"name\": \"Alice\"}");
            assert_eq!(unvalidated, "{\"name\": \"Alice\"}");

            let data = render_json(py, &template).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'name': 'Alice'}\n"),
                pyo3::ffi::c_str!("test_bindings_json_validation_text_expected.py"),
                pyo3::ffi::c_str!("test_bindings_json_validation_text_expected"),
            )
            .unwrap();
            assert!(
                data.bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
        });
    }

    #[test]
    fn bindings_surface_toml_validation_text_and_data() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("name='Alice'\ntemplate=t'name = {name}'\n"),
                pyo3::ffi::c_str!("test_bindings_toml_validation_text.py"),
                pyo3::ffi::c_str!("test_bindings_toml_validation_text"),
            )
            .unwrap();

            let template = module.getattr("template").unwrap();
            let validated = render_toml_text(py, &template).unwrap();
            let unvalidated = render_toml_text(py, &template).unwrap();
            assert_eq!(validated, "name = \"Alice\"");
            assert_eq!(unvalidated, "name = \"Alice\"");

            let data = render_toml(py, &template).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'name': 'Alice'}\n"),
                pyo3::ffi::c_str!("test_bindings_toml_validation_text_expected.py"),
                pyo3::ffi::c_str!("test_bindings_toml_validation_text_expected"),
            )
            .unwrap();
            assert!(
                data.bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
        });
    }

    #[test]
    fn bindings_surface_yaml_streams_and_exact_text() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "stream=t'---\\nname: Alice\\n---\\nname: Bob\\n'\nexplicit_end=t'---\\na: 1\\n...\\n---\\nb: 2\\n'\ncustom_mapping=t'value: !custom 3\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_streams.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_streams"),
            )
            .unwrap();

            let stream = render_yaml(py, &module.getattr("stream").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=[{'name': 'Alice'}, {'name': 'Bob'}]\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_streams_expected.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_streams_expected"),
            )
            .unwrap();
            assert!(
                stream
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let explicit_end =
                render_yaml_text(py, &module.getattr("explicit_end").unwrap()).unwrap();
            assert_eq!(explicit_end, "---\na: 1\n...\n---\nb: 2");

            let custom_mapping =
                render_yaml_text(py, &module.getattr("custom_mapping").unwrap()).unwrap();
            assert_eq!(custom_mapping, "value: !custom 3");
        });
    }

    #[test]
    fn bindings_surface_yaml_comment_only_documents_and_indent_indicator_text() {
        Python::with_gil(|py| {
            let is_empty_yaml = |value: &Py<PyAny>| {
                value.bind(py).is_none()
                    || value
                        .bind(py)
                        .downcast::<pyo3::types::PyList>()
                        .map(|items| items.is_empty())
                        .unwrap_or(false)
            };
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "comment_only=t'# comment\\n'\ncomment_only_explicit=t'--- # comment\\n'\ncomment_only_explicit_end=t'--- # comment\\n...\\n'\ncomment_only_stream=t'--- # comment\\n...\\n---\\na: 1\\n'\ncomment_only_mid_stream=t'---\\na: 1\\n--- # comment\\n...\\n---\\nb: 2\\n'\nindent_indicator=t'value: |1\\n a\\n b\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_comment_docs.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_comment_docs"),
            )
            .unwrap();

            let comment_only = render_yaml(py, &module.getattr("comment_only").unwrap()).unwrap();
            assert!(is_empty_yaml(&comment_only));

            let comment_only_explicit =
                render_yaml(py, &module.getattr("comment_only_explicit").unwrap()).unwrap();
            assert!(is_empty_yaml(&comment_only_explicit));

            let comment_only_explicit_end =
                render_yaml(py, &module.getattr("comment_only_explicit_end").unwrap()).unwrap();
            assert!(is_empty_yaml(&comment_only_explicit_end));

            let comment_only_stream =
                render_yaml(py, &module.getattr("comment_only_stream").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=[None, {'a': 1}]\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_comment_docs_expected.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_comment_docs_expected"),
            )
            .unwrap();
            assert!(
                comment_only_stream
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let comment_only_mid_stream =
                render_yaml(py, &module.getattr("comment_only_mid_stream").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=[{'a': 1}, None, {'b': 2}]\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_comment_docs_expected_mid.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_comment_docs_expected_mid"),
            )
            .unwrap();
            assert!(
                comment_only_mid_stream
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let comment_only_explicit_text =
                render_yaml_text(py, &module.getattr("comment_only_explicit").unwrap()).unwrap();
            assert_eq!(comment_only_explicit_text, "---\nnull");

            let comment_only_explicit_end_text =
                render_yaml_text(py, &module.getattr("comment_only_explicit_end").unwrap())
                    .unwrap();
            assert_eq!(comment_only_explicit_end_text, "---\nnull\n...");

            let comment_only_mid_stream_text =
                render_yaml_text(py, &module.getattr("comment_only_mid_stream").unwrap()).unwrap();
            assert_eq!(
                comment_only_mid_stream_text,
                "---\na: 1\n---\nnull\n...\n---\nb: 2"
            );

            let indent_indicator =
                render_yaml_text(py, &module.getattr("indent_indicator").unwrap()).unwrap();
            assert_eq!(indent_indicator, "value: |1\n a\n b\n");
        });
    }

    #[test]
    fn bindings_surface_yaml_root_tag_and_anchor_text() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "root_int=t'--- !!int 3\\n'\nroot_anchor=t'--- &root\\n  a: 1\\n'\nroot_anchor_sequence=t'--- &root\\n  - 1\\n  - 2\\n'\nexplicit_end=t'---\\na: 1\\n...\\n---\\nb: 2\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_exact_text.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_exact_text"),
            )
            .unwrap();

            let root_int = render_yaml_text(py, &module.getattr("root_int").unwrap()).unwrap();
            assert_eq!(root_int, "---\n!!int 3");

            let root_anchor =
                render_yaml_text(py, &module.getattr("root_anchor").unwrap()).unwrap();
            assert_eq!(root_anchor, "---\n&root\na: 1");

            let root_anchor_data =
                render_yaml(py, &module.getattr("root_anchor").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'a': 1}\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_exact_text_expected_anchor_map.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_exact_text_expected_anchor_map"),
            )
            .unwrap();
            assert!(
                root_anchor_data
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let root_anchor_sequence =
                render_yaml_text(py, &module.getattr("root_anchor_sequence").unwrap()).unwrap();
            assert_eq!(root_anchor_sequence, "---\n&root\n- 1\n- 2");

            let root_anchor_sequence_data =
                render_yaml(py, &module.getattr("root_anchor_sequence").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=[1, 2]\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_exact_text_expected_anchor_seq.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_exact_text_expected_anchor_seq"),
            )
            .unwrap();
            assert!(
                root_anchor_sequence_data
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let explicit_end =
                render_yaml_text(py, &module.getattr("explicit_end").unwrap()).unwrap();
            assert_eq!(explicit_end, "---\na: 1\n...\n---\nb: 2");

            let explicit_end_data =
                render_yaml(py, &module.getattr("explicit_end").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=[{'a': 1}, {'b': 2}]\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_exact_text_expected_stream.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_exact_text_expected_stream"),
            )
            .unwrap();
            assert!(
                explicit_end_data
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
        });
    }

    #[test]
    fn bindings_surface_toml_special_float_shapes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "template=t'array = [+inf, -inf, nan]\\nspecial_float_inline_table = {{ pos = +inf, neg = -inf, nan = nan }}\\nspecial_float_mixed_nested = [[+inf, -inf], [nan]]\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_special_float_shapes.py"),
                pyo3::ffi::c_str!("test_bindings_toml_special_float_shapes"),
            )
            .unwrap();

            let data = render_toml(py, &module.getattr("template").unwrap()).unwrap();
            let data = data.bind(py);

            let array = data.get_item("array").unwrap();
            let array = array.extract::<Vec<f64>>().unwrap();
            assert!(array[0].is_infinite() && array[0].is_sign_positive());
            assert!(array[1].is_infinite() && array[1].is_sign_negative());
            assert!(array[2].is_nan());

            let inline_table = data.get_item("special_float_inline_table").unwrap();
            let pos = inline_table
                .get_item("pos")
                .unwrap()
                .extract::<f64>()
                .unwrap();
            let neg = inline_table
                .get_item("neg")
                .unwrap()
                .extract::<f64>()
                .unwrap();
            let nan = inline_table
                .get_item("nan")
                .unwrap()
                .extract::<f64>()
                .unwrap();
            assert!(pos.is_infinite() && pos.is_sign_positive());
            assert!(neg.is_infinite() && neg.is_sign_negative());
            assert!(nan.is_nan());

            let nested = data
                .get_item("special_float_mixed_nested")
                .unwrap()
                .extract::<Vec<Vec<f64>>>()
                .unwrap();
            assert!(nested[0][0].is_infinite() && nested[0][0].is_sign_positive());
            assert!(nested[0][1].is_infinite() && nested[0][1].is_sign_negative());
            assert!(nested[1][0].is_nan());
        });
    }

    #[test]
    fn bindings_surface_json_fragment_render_text() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "left='prefix'\nright='suffix'\ntemplate=t'{{\"label\": {left}-{right}}}'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_fragment_text.py"),
                pyo3::ffi::c_str!("test_bindings_json_fragment_text"),
            )
            .unwrap();

            let rendered = render_json_text(py, &module.getattr("template").unwrap()).unwrap();
            assert_eq!(rendered, "{\"label\": \"prefix-suffix\"}");
        });
    }

    #[test]
    fn bindings_surface_toml_special_float_render_text() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "template=t'pos = {float(\"inf\")}\\nplus_inf = +inf\\nneg = {float(\"-inf\")}\\nvalue = {float(\"nan\")}\\nspecial_float_inline_table = {{ pos = +inf, neg = -inf, nan = nan }}\\nspecial_float_mixed_nested = [[+inf, -inf], [nan]]\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_special_float_text.py"),
                pyo3::ffi::c_str!("test_bindings_toml_special_float_text"),
            )
            .unwrap();

            let rendered = render_toml_text(py, &module.getattr("template").unwrap()).unwrap();
            assert_eq!(
                rendered,
                "pos = inf\nplus_inf = +inf\nneg = -inf\nvalue = nan\nspecial_float_inline_table = { pos = +inf, neg = -inf, nan = nan }\nspecial_float_mixed_nested = [[+inf, -inf], [nan]]"
            );
        });
    }

    #[test]
    fn bindings_surface_render_yamlag_directive_and_root_anchor_sequence_text() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "directive=t'%TAG !e! tag:example.com,2020:\\n---\\nvalue: !e!foo 1\\n'\nroot_anchor_sequence=t'--- &root\\n  - 1\\n  - 2\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_render_yamlext_surface.py"),
                pyo3::ffi::c_str!("test_bindings_render_yamlext_surface"),
            )
            .unwrap();

            let directive = render_yaml_text(py, &module.getattr("directive").unwrap()).unwrap();
            assert_eq!(
                directive,
                "%TAG !e! tag:example.com,2020:\n---\nvalue: !e!foo 1"
            );

            let root_anchor_sequence =
                render_yaml_text(py, &module.getattr("root_anchor_sequence").unwrap()).unwrap();
            assert_eq!(root_anchor_sequence, "---\n&root\n- 1\n- 2");
        });
    }

    #[test]
    fn bindings_surface_render_yamlag_directive_data_shapes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "directive=t'%TAG !e! tag:example.com,2020:\\n---\\nvalue: !e!foo 1\\n'\nroot=t'%YAML 1.2\\n%TAG !e! tag:example.com,2020:\\n---\\n!e!root {{value: !e!leaf 1}}\\n'\nroot_comment=t'%YAML 1.2\\n%TAG !e! tag:example.com,2020:\\n--- # comment\\n!e!root {{value: !e!leaf 1}}\\n'\nverbatim_root_mapping=t'--- !<tag:yaml.org,2002:map>\\na: 1\\n'\nverbatim_root_sequence=t'--- !<tag:yaml.org,2002:seq>\\n- 1\\n- 2\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_render_yamlag_directive_data.py"),
                pyo3::ffi::c_str!("test_bindings_render_yamlag_directive_data"),
            )
            .unwrap();

            for name in ["directive", "root", "root_comment"] {
                let actual = render_yaml(py, &module.getattr(name).unwrap()).unwrap();
                let expected = PyModule::from_code(
                    py,
                    pyo3::ffi::c_str!("expected={'value': 1}\n"),
                    pyo3::ffi::c_str!("test_bindings_render_yamlag_directive_data_expected.py"),
                    pyo3::ffi::c_str!("test_bindings_render_yamlag_directive_data_expected"),
                )
                .unwrap();
                assert!(
                    actual
                        .bind(py)
                        .eq(expected.getattr("expected").unwrap())
                        .unwrap(),
                    "{name}"
                );
            }

            let verbatim_root_mapping =
                render_yaml(py, &module.getattr("verbatim_root_mapping").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'a': 1}\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_verbatim_root_map_expected.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_verbatim_root_map_expected"),
            )
            .unwrap();
            assert!(
                verbatim_root_mapping
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let verbatim_root_sequence =
                render_yaml(py, &module.getattr("verbatim_root_sequence").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=[1, 2]\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_verbatim_root_seq_expected.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_verbatim_root_seq_expected"),
            )
            .unwrap();
            assert!(
                verbatim_root_sequence
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
        });
    }

    #[test]
    fn bindings_surface_json_unicode_and_escape_data() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "surrogate=t'\"\\\\uD834\\\\uDD1E\"'\ntemplate=t'{{\"int\": -0, \"exp\": 1.5e2, \"escapes\": \"\\\\b\\\\f\\\\n\\\\r\\\\t\\\\/\", \"unicode\": \"\\\\u00DF\\\\u6771\\\\uD834\\\\uDD1E\"}}'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_unicode_data.py"),
                pyo3::ffi::c_str!("test_bindings_json_unicode_data"),
            )
            .unwrap();

            let surrogate = render_json(py, &module.getattr("surrogate").unwrap()).unwrap();
            assert_eq!(surrogate.bind(py).extract::<String>().unwrap(), "𝄞");

            let actual = render_json(py, &module.getattr("template").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "expected={'int': 0, 'exp': 150.0, 'escapes': '\\b\\f\\n\\r\\t/', 'unicode': 'ß東𝄞'}\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_unicode_data_expected.py"),
                pyo3::ffi::c_str!("test_bindings_json_unicode_data_expected"),
            )
            .unwrap();
            assert!(
                actual
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
        });
    }

    #[test]
    fn bindings_surface_render_tomlemporal_render_text() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from datetime import date, time\nday=date(2024, 1, 2)\nmoment=time(4, 5, 6)\ntemplate=t'day = {day}\\nmoment = {moment}'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_render_tomlemporal_text.py"),
                pyo3::ffi::c_str!("test_bindings_render_tomlemporal_text"),
            )
            .unwrap();

            let rendered = render_toml_text(py, &module.getattr("template").unwrap()).unwrap();
            assert_eq!(rendered, "day = 2024-01-02\nmoment = 04:05:06");
        });
    }

    #[test]
    fn bindings_surface_yaml_explicit_end_comment_stream_and_tag_root_data() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "explicit_end_comment_stream=t'---\\na: 1\\n...\\n---\\nb: 2\\n'\ntag_root=t'%YAML 1.2\\n%TAG !e! tag:example.com,2020:\\n---\\n!e!root {{value: !e!leaf 1}}\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_stream_data.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_stream_data"),
            )
            .unwrap();

            let explicit_end_comment_stream =
                render_yaml(py, &module.getattr("explicit_end_comment_stream").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=[{'a': 1}, {'b': 2}]\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_stream_data_expected.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_stream_data_expected"),
            )
            .unwrap();
            assert!(
                explicit_end_comment_stream
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let tag_root = render_yaml(py, &module.getattr("tag_root").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'value': 1}\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_stream_data_expected_root.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_stream_data_expected_root"),
            )
            .unwrap();
            assert!(
                tag_root
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
        });
    }

    #[test]
    fn bindings_surface_json_whitespace_and_escape_arrays() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "top_bool_ws=t' \\n true \\t '\ntop_null_ws=t' \\r\\n null \\n'\narray=t'[\"\", 0, false, null, {{}}, []]'\nescapes=t'[\"\\\\u2028\", \"\\\\u2029\", \"\\\\/\", \"\\\\u005C\"]'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_whitespace.py"),
                pyo3::ffi::c_str!("test_bindings_json_whitespace"),
            )
            .unwrap();

            let top_bool_ws = render_json(py, &module.getattr("top_bool_ws").unwrap()).unwrap();
            assert!(top_bool_ws.bind(py).is_truthy().unwrap());

            let top_null_ws = render_json(py, &module.getattr("top_null_ws").unwrap()).unwrap();
            assert!(top_null_ws.bind(py).is_none());

            let array = render_json(py, &module.getattr("array").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=['', 0, False, None, {}, []]\n"),
                pyo3::ffi::c_str!("test_bindings_json_whitespace_expected.py"),
                pyo3::ffi::c_str!("test_bindings_json_whitespace_expected"),
            )
            .unwrap();
            assert!(
                array
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let escapes = render_json(py, &module.getattr("escapes").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=['\\u2028', '\\u2029', '/', '\\\\']\n"),
                pyo3::ffi::c_str!("test_bindings_json_whitespace_expected_escapes.py"),
                pyo3::ffi::c_str!("test_bindings_json_whitespace_expected_escapes"),
            )
            .unwrap();
            assert!(
                escapes
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
        });
    }

    #[test]
    fn bindings_surface_toml_string_families_data() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "value='name'\nbasic=t'basic = \"hi-{value}\"'\nliteral=t\"literal = 'hi-{value}'\"\nmulti_basic=t'multi_basic = \"\"\"hi-{value}\"\"\"'\nmulti_literal=t\"\"\"multi_literal = '''hi-{value}'''\"\"\"\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_string_families.py"),
                pyo3::ffi::c_str!("test_bindings_toml_string_families"),
            )
            .unwrap();

            let basic = render_toml(py, &module.getattr("basic").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'basic': 'hi-name'}\n"),
                pyo3::ffi::c_str!("test_bindings_toml_string_families_expected_basic.py"),
                pyo3::ffi::c_str!("test_bindings_toml_string_families_expected_basic"),
            )
            .unwrap();
            assert!(
                basic
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let literal = render_toml(py, &module.getattr("literal").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'literal': 'hi-name'}\n"),
                pyo3::ffi::c_str!("test_bindings_toml_string_families_expected_literal.py"),
                pyo3::ffi::c_str!("test_bindings_toml_string_families_expected_literal"),
            )
            .unwrap();
            assert!(
                literal
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let multi_basic = render_toml(py, &module.getattr("multi_basic").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'multi_basic': 'hi-name'}\n"),
                pyo3::ffi::c_str!("test_bindings_toml_string_families_expected_multi_basic.py"),
                pyo3::ffi::c_str!("test_bindings_toml_string_families_expected_multi_basic"),
            )
            .unwrap();
            assert!(
                multi_basic
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let multi_literal = render_toml(py, &module.getattr("multi_literal").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'multi_literal': 'hi-name'}\n"),
                pyo3::ffi::c_str!("test_bindings_toml_string_families_expected_multi_literal.py"),
                pyo3::ffi::c_str!("test_bindings_toml_string_families_expected_multi_literal"),
            )
            .unwrap();
            assert!(
                multi_literal
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
        });
    }

    #[test]
    fn bindings_surface_yaml_quoted_scalars_and_core_schema() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "quoted=t'value: \"line\\\\nnext \\\\u03B1 \\\\x41\"'\nspecial=t'value: \"\\\\N\"'\nnbsp=t'value: \"\\\\_\"'\ncore=t'on: on\\nyes: yes\\ntruth: true\\nempty: null\\n'\nsequence=t'- Alice\\n- true\\n- on\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_scalars.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_scalars"),
            )
            .unwrap();

            let quoted = render_yaml(py, &module.getattr("quoted").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'value': 'line\\nnext α A'}\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_scalars_expected_quoted.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_scalars_expected_quoted"),
            )
            .unwrap();
            assert!(
                quoted
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let special = render_yaml(py, &module.getattr("special").unwrap()).unwrap();
            let nbsp = render_yaml(py, &module.getattr("nbsp").unwrap()).unwrap();
            assert_eq!(
                special
                    .bind(py)
                    .get_item("value")
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "\u{85}"
            );
            assert_eq!(
                nbsp.bind(py)
                    .get_item("value")
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "\u{A0}"
            );

            let core = render_yaml(py, &module.getattr("core").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "expected={'on': 'on', 'yes': 'yes', 'truth': True, 'empty': None}\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_scalars_expected_core.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_scalars_expected_core"),
            )
            .unwrap();
            assert!(
                core.bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let sequence = render_yaml(py, &module.getattr("sequence").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=['Alice', True, 'on']\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_scalars_expected_sequence.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_scalars_expected_sequence"),
            )
            .unwrap();
            assert!(
                sequence
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
        });
    }

    #[test]
    fn bindings_surface_yaml_quoted_scalar_escape_and_folding_families() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ndouble=t'value: \"line\\\\nnext \\\\u03B1 \\\\x41\"'\nsingle=t\"value: 'it''s ok'\"\nempty_single=Template(\"value: ''\\n\")\nempty_double=t'value: \"\"'\nsingle_blank=Template(\"value: 'a\\n\\n  b\\n\\n  c'\\n\")\nmultiline_double=t'value: \"a\\n  b\"'\nmultiline_double_blank=t'value: \"a\\n\\n  b\"'\nmultiline_double_more_blank=t'value: \"a\\n\\n\\n  b\"'\nunicode_upper=t'value: \"\\\\U0001D11E\"'\ncrlf_join=Template('value: \"a\\\\\\r\\n  b\"\\n')\nnel=t'value: \"\\\\N\"'\nnbsp=t'value: \"\\\\_\"'\nspace=t'value: \"\\\\ \"'\nslash=t'value: \"\\\\/\"'\ntab=t'value: \"\\\\t\"'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_quoted_scalar_families.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_quoted_scalar_families"),
            )
            .unwrap();

            for (name, expected_src, file_name, module_name) in [
                (
                    "double",
                    "expected={'value': 'line\\nnext α A'}\n",
                    "test_bindings_yaml_quoted_scalar_families_expected_double.py",
                    "test_bindings_yaml_quoted_scalar_families_expected_double",
                ),
                (
                    "single",
                    "expected={'value': \"it\\'s ok\"}\n",
                    "test_bindings_yaml_quoted_scalar_families_expected_single.py",
                    "test_bindings_yaml_quoted_scalar_families_expected_single",
                ),
                (
                    "empty_single",
                    "expected={'value': ''}\n",
                    "test_bindings_yaml_quoted_scalar_families_expected_empty_single.py",
                    "test_bindings_yaml_quoted_scalar_families_expected_empty_single",
                ),
                (
                    "empty_double",
                    "expected={'value': ''}\n",
                    "test_bindings_yaml_quoted_scalar_families_expected_empty_double.py",
                    "test_bindings_yaml_quoted_scalar_families_expected_empty_double",
                ),
                (
                    "single_blank",
                    "expected={'value': 'a\\nb\\nc'}\n",
                    "test_bindings_yaml_quoted_scalar_families_expected_single_blank.py",
                    "test_bindings_yaml_quoted_scalar_families_expected_single_blank",
                ),
                (
                    "multiline_double",
                    "expected={'value': 'a b'}\n",
                    "test_bindings_yaml_quoted_scalar_families_expected_multi.py",
                    "test_bindings_yaml_quoted_scalar_families_expected_multi",
                ),
                (
                    "multiline_double_blank",
                    "expected={'value': 'a\\nb'}\n",
                    "test_bindings_yaml_quoted_scalar_families_expected_multi_blank.py",
                    "test_bindings_yaml_quoted_scalar_families_expected_multi_blank",
                ),
                (
                    "multiline_double_more_blank",
                    "expected={'value': 'a\\n\\nb'}\n",
                    "test_bindings_yaml_quoted_scalar_families_expected_multi_more_blank.py",
                    "test_bindings_yaml_quoted_scalar_families_expected_multi_more_blank",
                ),
                (
                    "unicode_upper",
                    "expected={'value': '𝄞'}\n",
                    "test_bindings_yaml_quoted_scalar_families_expected_unicode.py",
                    "test_bindings_yaml_quoted_scalar_families_expected_unicode",
                ),
                (
                    "crlf_join",
                    "expected={'value': 'ab'}\n",
                    "test_bindings_yaml_quoted_scalar_families_expected_crlf.py",
                    "test_bindings_yaml_quoted_scalar_families_expected_crlf",
                ),
                (
                    "space",
                    "expected={'value': ' '}\n",
                    "test_bindings_yaml_quoted_scalar_families_expected_space.py",
                    "test_bindings_yaml_quoted_scalar_families_expected_space",
                ),
                (
                    "slash",
                    "expected={'value': '/'}\n",
                    "test_bindings_yaml_quoted_scalar_families_expected_slash.py",
                    "test_bindings_yaml_quoted_scalar_families_expected_slash",
                ),
                (
                    "tab",
                    "expected={'value': '\\t'}\n",
                    "test_bindings_yaml_quoted_scalar_families_expected_tab.py",
                    "test_bindings_yaml_quoted_scalar_families_expected_tab",
                ),
            ] {
                let actual = render_yaml(py, &module.getattr(name).unwrap()).unwrap();
                let expected = PyModule::from_code(
                    py,
                    CString::new(expected_src).unwrap().as_c_str(),
                    CString::new(file_name).unwrap().as_c_str(),
                    CString::new(module_name).unwrap().as_c_str(),
                )
                .unwrap();
                assert!(
                    actual
                        .bind(py)
                        .eq(expected.getattr("expected").unwrap())
                        .unwrap(),
                    "{name}"
                );
            }

            let nel = render_yaml(py, &module.getattr("nel").unwrap()).unwrap();
            let nbsp = render_yaml(py, &module.getattr("nbsp").unwrap()).unwrap();
            assert_eq!(
                nel.bind(py)
                    .get_item("value")
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "\u{85}"
            );
            assert_eq!(
                nbsp.bind(py)
                    .get_item("value")
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "\u{A0}"
            );

            for (name, expected_text) in [
                ("double", "value: \"line\\nnext α A\""),
                ("single", "value: 'it''s ok'"),
                ("empty_single", "value: ''"),
                ("empty_double", "value: \"\""),
                ("single_blank", "value: 'a\n\n  b\n\n  c'"),
                ("multiline_double", "value: \"a b\""),
                ("multiline_double_blank", "value: \"a\\nb\""),
                ("multiline_double_more_blank", "value: \"a\\n\\nb\""),
                ("unicode_upper", "value: \"𝄞\""),
                ("crlf_join", "value: \"ab\""),
                ("nel", "value: \"\u{0085}\""),
                ("nbsp", "value: \"\u{00A0}\""),
                ("space", "value: \" \""),
                ("slash", "value: \"/\""),
                ("tab", "value: \"\\t\""),
            ] {
                let rendered = render_yaml_text(py, &module.getattr(name).unwrap()).unwrap();
                assert_eq!(rendered, expected_text, "{name}");
            }
        });
    }

    #[test]
    fn bindings_surface_yaml_spec_quoted_scalar_examples_round_trip() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "unicode=t'unicode: \"Sosa did fine.\\\\u263A\"'\ncontrol=t'control: \"\\\\b1998\\\\t1999\\\\t2000\\\\n\"'\nsingle=t'''single: '\"Howdy!\" he cried.' '''\nquoted=t'''quoted: ' # Not a ''comment''.' '''\ntie=t'''tie: '|\\\\-*-/|' '''\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_spec_quoted_examples.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_spec_quoted_examples"),
            )
            .unwrap();

            for (name, expected_src, file_name, module_name) in [
                (
                    "unicode",
                    "expected={'unicode': 'Sosa did fine.☺'}\n",
                    "test_bindings_yaml_spec_quoted_examples_expected_unicode.py",
                    "test_bindings_yaml_spec_quoted_examples_expected_unicode",
                ),
                (
                    "control",
                    "expected={'control': '\\b1998\\t1999\\t2000\\n'}\n",
                    "test_bindings_yaml_spec_quoted_examples_expected_control.py",
                    "test_bindings_yaml_spec_quoted_examples_expected_control",
                ),
                (
                    "single",
                    "expected={'single': '\"Howdy!\" he cried.'}\n",
                    "test_bindings_yaml_spec_quoted_examples_expected_single.py",
                    "test_bindings_yaml_spec_quoted_examples_expected_single",
                ),
                (
                    "quoted",
                    "expected={'quoted': \" # Not a 'comment'.\"}\n",
                    "test_bindings_yaml_spec_quoted_examples_expected_quoted.py",
                    "test_bindings_yaml_spec_quoted_examples_expected_quoted",
                ),
                (
                    "tie",
                    "expected={'tie': '|\\\\-*-/|'}\n",
                    "test_bindings_yaml_spec_quoted_examples_expected_tie.py",
                    "test_bindings_yaml_spec_quoted_examples_expected_tie",
                ),
            ] {
                let actual = render_yaml(py, &module.getattr(name).unwrap()).unwrap();
                let expected = PyModule::from_code(
                    py,
                    CString::new(expected_src).unwrap().as_c_str(),
                    CString::new(file_name).unwrap().as_c_str(),
                    CString::new(module_name).unwrap().as_c_str(),
                )
                .unwrap();
                assert!(
                    actual
                        .bind(py)
                        .eq(expected.getattr("expected").unwrap())
                        .unwrap(),
                    "{name}"
                );
            }

            for (name, expected_text) in [
                ("unicode", "unicode: \"Sosa did fine.☺\""),
                ("control", "control: \"\\b1998\\t1999\\t2000\\n\""),
                ("single", "single: '\"Howdy!\" he cried.'"),
                ("quoted", "quoted: ' # Not a ''comment''.'"),
                ("tie", "tie: '|\\-*-/|'"),
            ] {
                let rendered = render_yaml_text(py, &module.getattr(name).unwrap()).unwrap();
                assert_eq!(rendered, expected_text, "{name}");
            }
        });
    }

    #[test]
    fn bindings_surface_json_number_type_shapes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "upper_exp=t'1E2'\nupper_exp_plus=t'1E+2'\nneg_exp_zero=t'-1e-0'\nupper_exp_negative_zero=t'1E-0'\nexp_with_fraction_zero=t'1.0e-0'\nnegative_zero_exp_upper=t'-0E0'\nnested=t'{{\"a\": [0, -0, -0.0, 1e0, -1E-0]}}'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_number_shapes.py"),
                pyo3::ffi::c_str!("test_bindings_json_number_shapes"),
            )
            .unwrap();

            for (name, expected_repr, expected_type) in [
                ("upper_exp", "100.0", "float"),
                ("upper_exp_plus", "100.0", "float"),
                ("neg_exp_zero", "-1.0", "float"),
                ("upper_exp_negative_zero", "1.0", "float"),
                ("exp_with_fraction_zero", "1.0", "float"),
                ("negative_zero_exp_upper", "-0.0", "float"),
            ] {
                let value = render_json(py, &module.getattr(name).unwrap()).unwrap();
                let repr_fn = py.import("builtins").unwrap().getattr("repr").unwrap();
                let actual_repr = repr_fn
                    .call1((value.bind(py),))
                    .unwrap()
                    .extract::<String>()
                    .unwrap();
                let actual_type = value.bind(py).get_type().name().unwrap().to_string();
                assert_eq!(actual_repr, expected_repr);
                assert_eq!(actual_type, expected_type);
            }

            let nested = render_json(py, &module.getattr("nested").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'a': [0, 0, -0.0, 1.0, -1.0]}\n"),
                pyo3::ffi::c_str!("test_bindings_json_number_shapes_expected.py"),
                pyo3::ffi::c_str!("test_bindings_json_number_shapes_expected"),
            )
            .unwrap();
            assert!(
                nested
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
        });
    }

    #[test]
    fn bindings_surface_toml_multiline_string_rules() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "trimmed=t'value = \"\"\"\\nalpha\\\\\\n  beta\\n\"\"\"'\ncrlf=t'value = \"\"\"\\r\\na\\\\\\r\\n  b\\r\\n\"\"\"\\n'\nbasic_one=t'value = \"\"\"\"\"\"\"\\n'\nbasic_two=t'value = \"\"\"\"\"\"\"\"\\n'\nliteral_one=t\"\"\"value = '''''''\\n\"\"\"\nliteral_two=t\"\"\"value = ''''''''\\n\"\"\"\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_multiline_strings.py"),
                pyo3::ffi::c_str!("test_bindings_toml_multiline_strings"),
            )
            .unwrap();

            for (name, expected_src, file_name, module_name) in [
                (
                    "trimmed",
                    "expected={'value': 'alphabeta\\n'}\n",
                    "test_bindings_toml_multiline_strings_expected_trimmed.py",
                    "test_bindings_toml_multiline_strings_expected_trimmed",
                ),
                (
                    "crlf",
                    "expected={'value': 'ab\\n'}\n",
                    "test_bindings_toml_multiline_strings_expected_crlf.py",
                    "test_bindings_toml_multiline_strings_expected_crlf",
                ),
                (
                    "basic_one",
                    "expected={'value': '\"'}\n",
                    "test_bindings_toml_multiline_strings_expected_basic_one.py",
                    "test_bindings_toml_multiline_strings_expected_basic_one",
                ),
                (
                    "basic_two",
                    "expected={'value': '\"\"'}\n",
                    "test_bindings_toml_multiline_strings_expected_basic_two.py",
                    "test_bindings_toml_multiline_strings_expected_basic_two",
                ),
                (
                    "literal_one",
                    "expected={'value': \"'\"}\n",
                    "test_bindings_toml_multiline_strings_expected_literal_one.py",
                    "test_bindings_toml_multiline_strings_expected_literal_one",
                ),
                (
                    "literal_two",
                    "expected={'value': \"''\"}\n",
                    "test_bindings_toml_multiline_strings_expected_literal_two.py",
                    "test_bindings_toml_multiline_strings_expected_literal_two",
                ),
            ] {
                let actual = render_toml(py, &module.getattr(name).unwrap()).unwrap();
                let expected = PyModule::from_code(
                    py,
                    CString::new(expected_src).unwrap().as_c_str(),
                    CString::new(file_name).unwrap().as_c_str(),
                    CString::new(module_name).unwrap().as_c_str(),
                )
                .unwrap();
                assert!(
                    actual
                        .bind(py)
                        .eq(expected.getattr("expected").unwrap())
                        .unwrap()
                );
            }
        });
    }

    #[test]
    fn bindings_surface_yaml_block_scalars_and_plain_folding() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "literal_keep=t'value: |+\\n  a\\n  b\\n'\nfolded_more=t'value: >\\n  a\\n    b\\n  c\\n'\nindent_indicator=t'value: |2\\n  a\\n  b\\n'\nplain_mapping=t'value: a\\n  b\\n  c\\n'\nplain_blank=t'value: a\\n\\n  b\\n'\nrendered_blank=t'value: a\\n\\n  b\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_block_scalars.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_block_scalars"),
            )
            .unwrap();

            for (name, expected_src, file_name, module_name) in [
                (
                    "literal_keep",
                    "expected={'value': 'a\\nb\\n'}\n",
                    "test_bindings_yaml_block_scalars_expected_literal_keep.py",
                    "test_bindings_yaml_block_scalars_expected_literal_keep",
                ),
                (
                    "folded_more",
                    "expected={'value': 'a\\n  b\\nc\\n'}\n",
                    "test_bindings_yaml_block_scalars_expected_folded_more.py",
                    "test_bindings_yaml_block_scalars_expected_folded_more",
                ),
                (
                    "indent_indicator",
                    "expected={'value': 'a\\nb\\n'}\n",
                    "test_bindings_yaml_block_scalars_expected_indent.py",
                    "test_bindings_yaml_block_scalars_expected_indent",
                ),
                (
                    "plain_mapping",
                    "expected={'value': 'a b c'}\n",
                    "test_bindings_yaml_block_scalars_expected_plain_mapping.py",
                    "test_bindings_yaml_block_scalars_expected_plain_mapping",
                ),
                (
                    "plain_blank",
                    "expected={'value': 'a\\nb'}\n",
                    "test_bindings_yaml_block_scalars_expected_plain_blank.py",
                    "test_bindings_yaml_block_scalars_expected_plain_blank",
                ),
            ] {
                let actual = render_yaml(py, &module.getattr(name).unwrap()).unwrap();
                let expected = PyModule::from_code(
                    py,
                    CString::new(expected_src).unwrap().as_c_str(),
                    CString::new(file_name).unwrap().as_c_str(),
                    CString::new(module_name).unwrap().as_c_str(),
                )
                .unwrap();
                assert!(
                    actual
                        .bind(py)
                        .eq(expected.getattr("expected").unwrap())
                        .unwrap()
                );
            }

            let rendered_blank =
                render_yaml_text(py, &module.getattr("rendered_blank").unwrap()).unwrap();
            assert_eq!(rendered_blank, "value: \"a\\nb\"");
        });
    }

    #[test]
    fn bindings_surface_yaml_block_chomping_families() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "literal_strip=t'value: |-\\n  a\\n  b\\n'\nliteral_keep=t'value: |+\\n  a\\n  b\\n'\nliteral_keep_leading_blank=t'value: |+\\n\\n  a\\n'\nfolded_strip=t'value: >-\\n  a\\n  b\\n'\nfolded_keep=t'value: >+\\n  a\\n  b\\n'\nfolded_more=t'value: >\\n  a\\n    b\\n  c\\n'\nindent_indicator=t'value: |2\\n  a\\n  b\\n'\nliteral_blank_keep=t'value: |+\\n  a\\n\\n  b\\n'\nfolded_blank_keep=t'value: >+\\n  a\\n\\n  b\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_block_chomping_families.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_block_chomping_families"),
            )
            .unwrap();

            for (name, expected_src, file_name, module_name) in [
                (
                    "literal_strip",
                    "expected={'value': 'a\\nb'}\n",
                    "test_bindings_yaml_block_chomping_expected_literal_strip.py",
                    "test_bindings_yaml_block_chomping_expected_literal_strip",
                ),
                (
                    "literal_keep",
                    "expected={'value': 'a\\nb\\n'}\n",
                    "test_bindings_yaml_block_chomping_expected_literal_keep.py",
                    "test_bindings_yaml_block_chomping_expected_literal_keep",
                ),
                (
                    "literal_keep_leading_blank",
                    "expected={'value': '\\na\\n'}\n",
                    "test_bindings_yaml_block_chomping_expected_literal_leading.py",
                    "test_bindings_yaml_block_chomping_expected_literal_leading",
                ),
                (
                    "folded_strip",
                    "expected={'value': 'a b'}\n",
                    "test_bindings_yaml_block_chomping_expected_folded_strip.py",
                    "test_bindings_yaml_block_chomping_expected_folded_strip",
                ),
                (
                    "folded_keep",
                    "expected={'value': 'a b\\n'}\n",
                    "test_bindings_yaml_block_chomping_expected_folded_keep.py",
                    "test_bindings_yaml_block_chomping_expected_folded_keep",
                ),
                (
                    "folded_more",
                    "expected={'value': 'a\\n  b\\nc\\n'}\n",
                    "test_bindings_yaml_block_chomping_expected_folded_more.py",
                    "test_bindings_yaml_block_chomping_expected_folded_more",
                ),
                (
                    "indent_indicator",
                    "expected={'value': 'a\\nb\\n'}\n",
                    "test_bindings_yaml_block_chomping_expected_indent.py",
                    "test_bindings_yaml_block_chomping_expected_indent",
                ),
                (
                    "literal_blank_keep",
                    "expected={'value': 'a\\n\\nb\\n'}\n",
                    "test_bindings_yaml_block_chomping_expected_literal_blank.py",
                    "test_bindings_yaml_block_chomping_expected_literal_blank",
                ),
                (
                    "folded_blank_keep",
                    "expected={'value': 'a\\nb\\n'}\n",
                    "test_bindings_yaml_block_chomping_expected_folded_blank.py",
                    "test_bindings_yaml_block_chomping_expected_folded_blank",
                ),
            ] {
                let actual = render_yaml(py, &module.getattr(name).unwrap()).unwrap();
                let expected = PyModule::from_code(
                    py,
                    CString::new(expected_src).unwrap().as_c_str(),
                    CString::new(file_name).unwrap().as_c_str(),
                    CString::new(module_name).unwrap().as_c_str(),
                )
                .unwrap();
                assert!(
                    actual
                        .bind(py)
                        .eq(expected.getattr("expected").unwrap())
                        .unwrap(),
                    "{name}"
                );
            }

            for (name, expected_text) in [
                ("literal_strip", "value: |-\n  a\n  b"),
                ("literal_keep", "value: |+\n  a\n  b\n"),
                ("literal_keep_leading_blank", "value: |+\n  \n  a\n"),
                ("folded_strip", "value: >-\n  a\n  b"),
                ("folded_keep", "value: >+\n  a\n  b\n"),
                ("folded_more", "value: >\n  a\n    b\n  c\n"),
                ("indent_indicator", "value: |2\n  a\n  b\n"),
                ("literal_blank_keep", "value: |+\n  a\n  \n  b\n"),
                ("folded_blank_keep", "value: >+\n  a\n  \n  b\n"),
            ] {
                let rendered = render_yaml_text(py, &module.getattr(name).unwrap()).unwrap();
                assert_eq!(rendered, expected_text, "{name}");
            }
        });
    }

    #[test]
    fn bindings_surface_json_nested_unicode_and_whitespace_values() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "nested=t'{{\"x\": {{\"a\": \"\\\\u005C\", \"b\": \"\\\\u00DF\", \"c\": \"\\\\u2029\"}}}}'\narray=t'{{\"a\": [{{\"b\": \"\\\\u005C\", \"c\": \"\\\\u00DF\"}}]}}'\nws=t'\\n\\r\\t \"x\" \\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_nested_unicode.py"),
                pyo3::ffi::c_str!("test_bindings_json_nested_unicode"),
            )
            .unwrap();

            let nested = render_json(py, &module.getattr("nested").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'x': {'a': '\\\\', 'b': 'ß', 'c': '\\u2029'}}\n"),
                pyo3::ffi::c_str!("test_bindings_json_nested_unicode_expected.py"),
                pyo3::ffi::c_str!("test_bindings_json_nested_unicode_expected"),
            )
            .unwrap();
            assert!(
                nested
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let array = render_json(py, &module.getattr("array").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'a': [{'b': '\\\\', 'c': 'ß'}]}\n"),
                pyo3::ffi::c_str!("test_bindings_json_nested_unicode_array_expected.py"),
                pyo3::ffi::c_str!("test_bindings_json_nested_unicode_array_expected"),
            )
            .unwrap();
            assert!(
                array
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let ws = render_json(py, &module.getattr("ws").unwrap()).unwrap();
            assert_eq!(ws.bind(py).extract::<String>().unwrap(), "x");
        });
    }

    #[test]
    fn bindings_surface_toml_datetime_and_special_float_arrays() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "template=t'utc_fraction_lower_array = [2024-01-02T03:04:05.123456z, 2024-01-02T03:04:06z]\\narray_offset_fraction = [1979-05-27T07:32:00.999999-07:00, 1979-05-27T07:32:00Z]\\nsigned_int_array = [+1, +0, -1]\\nspecial_float_array = [+inf, -inf, nan]\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_render_tomlemporal_arrays.py"),
                pyo3::ffi::c_str!("test_bindings_render_tomlemporal_arrays"),
            )
            .unwrap();

            let rendered = render_toml_text(py, &module.getattr("template").unwrap()).unwrap();
            assert_eq!(
                rendered,
                "utc_fraction_lower_array = [2024-01-02T03:04:05.123456z, 2024-01-02T03:04:06z]\narray_offset_fraction = [1979-05-27T07:32:00.999999-07:00, 1979-05-27T07:32:00Z]\nsigned_int_array = [+1, +0, -1]\nspecial_float_array = [+inf, -inf, nan]"
            );

            let data = render_toml(py, &module.getattr("template").unwrap()).unwrap();
            let builtins = py.import("builtins").unwrap();
            let len_fn = builtins.getattr("len").unwrap();
            let signed_int_array = data.bind(py).get_item("signed_int_array").unwrap();
            let special_float_array = data.bind(py).get_item("special_float_array").unwrap();
            let utc_fraction_lower_array =
                data.bind(py).get_item("utc_fraction_lower_array").unwrap();
            let array_offset_fraction = data.bind(py).get_item("array_offset_fraction").unwrap();
            assert_eq!(
                len_fn
                    .call1((utc_fraction_lower_array,))
                    .unwrap()
                    .extract::<usize>()
                    .unwrap(),
                2
            );
            assert_eq!(
                len_fn
                    .call1((array_offset_fraction,))
                    .unwrap()
                    .extract::<usize>()
                    .unwrap(),
                2
            );
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=[1, 0, -1]\n"),
                pyo3::ffi::c_str!("test_bindings_render_tomlemporal_arrays_expected.py"),
                pyo3::ffi::c_str!("test_bindings_render_tomlemporal_arrays_expected"),
            )
            .unwrap();
            assert!(
                signed_int_array
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
            let math = py.import("math").unwrap();
            assert!(
                math.call_method1("isinf", (special_float_array.get_item(0).unwrap(),))
                    .unwrap()
                    .extract::<bool>()
                    .unwrap()
            );
            assert!(
                math.call_method1("isinf", (special_float_array.get_item(1).unwrap(),))
                    .unwrap()
                    .extract::<bool>()
                    .unwrap()
            );
            assert!(
                math.call_method1("isnan", (special_float_array.get_item(2).unwrap(),))
                    .unwrap()
                    .extract::<bool>()
                    .unwrap()
            );
        });
    }

    #[test]
    fn bindings_surface_yaml_merge_alias_and_explicit_core_tags() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "merge=t'base: &base\\n  a: 1\\n  b: 2\\nderived:\\n  <<: *base\\n  c: 3\\n'\nflow_alias=t'value: {{left: &a 1, right: *a}}\\n'\nroot_bool=t'--- !!bool true\\n'\nroot_str=t'--- !!str true\\n'\nroot_int_text=t'--- !!int 3\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_merge_and_tags.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_merge_and_tags"),
            )
            .unwrap();

            let merge = render_yaml(py, &module.getattr("merge").unwrap()).unwrap();
            let repr_fn = py.import("builtins").unwrap().getattr("repr").unwrap();
            let merge_repr = repr_fn
                .call1((merge.bind(py),))
                .unwrap()
                .extract::<String>()
                .unwrap();
            assert_eq!(
                merge_repr,
                "{'base': {'a': 1, 'b': 2}, 'derived': {'a': 1, 'b': 2, 'c': 3}}"
            );

            let flow_alias = render_yaml(py, &module.getattr("flow_alias").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'value': {'left': 1, 'right': 1}}\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_merge_and_tags_expected_alias.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_merge_and_tags_expected_alias"),
            )
            .unwrap();
            assert!(
                flow_alias
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let root_bool = render_yaml(py, &module.getattr("root_bool").unwrap()).unwrap();
            assert!(root_bool.bind(py).is_truthy().unwrap());

            let root_str = render_yaml(py, &module.getattr("root_str").unwrap()).unwrap();
            assert_eq!(root_str.bind(py).extract::<String>().unwrap(), "true");

            let root_int_text =
                render_yaml_text(py, &module.getattr("root_int_text").unwrap()).unwrap();
            assert_eq!(root_int_text, "---\n!!int 3");
        });
    }

    #[test]
    fn bindings_surface_json_empty_collections_and_nested_names() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nempty_object=Template(' \\n { } \\t ')\nempty_array=Template('[ \\n\\t ]')\nnested_empty_names=Template('{\"\": {\"\": []}}')\nnested_empty_name_array=Template('{\"\": [\"\", {\"\": 0}]}')\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_empty_collections.py"),
                pyo3::ffi::c_str!("test_bindings_json_empty_collections"),
            )
            .unwrap();

            let empty_object = render_json(py, &module.getattr("empty_object").unwrap()).unwrap();
            let empty_array = render_json(py, &module.getattr("empty_array").unwrap()).unwrap();
            let nested_empty_names =
                render_json(py, &module.getattr("nested_empty_names").unwrap()).unwrap();
            let nested_empty_name_array =
                render_json(py, &module.getattr("nested_empty_name_array").unwrap()).unwrap();

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={}\n"),
                pyo3::ffi::c_str!("test_bindings_json_empty_collections_expected_obj.py"),
                pyo3::ffi::c_str!("test_bindings_json_empty_collections_expected_obj"),
            )
            .unwrap();
            assert!(
                empty_object
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=[]\n"),
                pyo3::ffi::c_str!("test_bindings_json_empty_collections_expected_arr.py"),
                pyo3::ffi::c_str!("test_bindings_json_empty_collections_expected_arr"),
            )
            .unwrap();
            assert!(
                empty_array
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'': {'': []}}\n"),
                pyo3::ffi::c_str!("test_bindings_json_empty_collections_expected_nested.py"),
                pyo3::ffi::c_str!("test_bindings_json_empty_collections_expected_nested"),
            )
            .unwrap();
            assert!(
                nested_empty_names
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'': ['', {'': 0}]}\n"),
                pyo3::ffi::c_str!("test_bindings_json_empty_collections_expected_nested_arr.py"),
                pyo3::ffi::c_str!("test_bindings_json_empty_collections_expected_nested_arr"),
            )
            .unwrap();
            assert!(
                nested_empty_name_array
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
        });
    }

    #[test]
    fn bindings_surface_toml_empty_headers_and_inline_table_errors() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nheader=t'[\"\"]\\nvalue = 1\\n'\nheader_subtable=t'[\"\"]\\nvalue = 1\\n[\"\".inner]\\nname = \"x\"\\n'\ninline_invalid=Template('value = { a = 1,\\n b = 2 }\\n')\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_headers_and_inline_errors.py"),
                pyo3::ffi::c_str!("test_bindings_toml_headers_and_inline_errors"),
            )
            .unwrap();

            let header = render_toml(py, &module.getattr("header").unwrap()).unwrap();
            let header_subtable =
                render_toml(py, &module.getattr("header_subtable").unwrap()).unwrap();

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'': {'value': 1}}\n"),
                pyo3::ffi::c_str!("test_bindings_toml_headers_expected.py"),
                pyo3::ffi::c_str!("test_bindings_toml_headers_expected"),
            )
            .unwrap();
            assert!(
                header
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'': {'value': 1, 'inner': {'name': 'x'}}}\n"),
                pyo3::ffi::c_str!("test_bindings_toml_headers_subtable_expected.py"),
                pyo3::ffi::c_str!("test_bindings_toml_headers_subtable_expected"),
            )
            .unwrap();
            assert!(
                header_subtable
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let error = render_toml_text_for_profile(
                py,
                &module.getattr("inline_invalid").unwrap(),
                TomlProfile::V1_0,
            )
            .expect_err("inline tables must remain single-line");
            assert!(error.is_instance_of::<TemplateParseError>(py));
            assert!(
                error.to_string().contains("Expected a TOML key segment"),
                "unexpected error: {error}"
            );
        });
    }

    #[test]
    fn bindings_surface_yaml_flow_edge_cases_and_custom_tag_text() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nflow_seq=t'[1, 2,]\\n'\nflow_map=Template('{a: 1,}\\n')\nexplicit_key_seq=t'? a\\n: - 1\\n  - 2\\n'\ncustom_scalar=t'!custom 3\\n'\ncustom_mapping=t'value: !custom 3\\n'\ncommented_root_sequence=t'--- # comment\\n!custom [1, 2]\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_edges.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_edges"),
            )
            .unwrap();

            let flow_seq = render_yaml(py, &module.getattr("flow_seq").unwrap()).unwrap();
            let flow_map = render_yaml(py, &module.getattr("flow_map").unwrap()).unwrap();
            let explicit_key_seq =
                render_yaml(py, &module.getattr("explicit_key_seq").unwrap()).unwrap();
            let commented_root_sequence =
                render_yaml(py, &module.getattr("commented_root_sequence").unwrap()).unwrap();

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=[1, 2]\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_edges_expected_seq.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_edges_expected_seq"),
            )
            .unwrap();
            assert!(
                flow_seq
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
            assert!(
                commented_root_sequence
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'a': 1}\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_edges_expected_map.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_edges_expected_map"),
            )
            .unwrap();
            assert!(
                flow_map
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'a': [1, 2]}\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_edges_expected_key_seq.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_edges_expected_key_seq"),
            )
            .unwrap();
            assert!(
                explicit_key_seq
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let custom_scalar =
                render_yaml_text(py, &module.getattr("custom_scalar").unwrap()).unwrap();
            let custom_mapping =
                render_yaml_text(py, &module.getattr("custom_mapping").unwrap()).unwrap();
            assert_eq!(custom_scalar, "!custom 3");
            assert_eq!(custom_mapping, "value: !custom 3");
        });
    }

    #[test]
    fn bindings_surface_json_nested_keyword_and_collection_mix() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nmixed=Template('{\"a\": [true, false, null], \"b\": {\"c\": -1e-0}}')\nempty_mix=Template('{\"a\": [{}, [], \"\", 0, false, null]}')\narray_mix=Template('[{\"a\": []}, {\"b\": {}}, \"\", 0, false, null]')\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_keyword_mix.py"),
                pyo3::ffi::c_str!("test_bindings_json_keyword_mix"),
            )
            .unwrap();

            let mixed = render_json(py, &module.getattr("mixed").unwrap()).unwrap();
            let empty_mix = render_json(py, &module.getattr("empty_mix").unwrap()).unwrap();
            let array_mix = render_json(py, &module.getattr("array_mix").unwrap()).unwrap();

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'a': [True, False, None], 'b': {'c': -1.0}}\n"),
                pyo3::ffi::c_str!("test_bindings_json_keyword_mix_expected.py"),
                pyo3::ffi::c_str!("test_bindings_json_keyword_mix_expected"),
            )
            .unwrap();
            assert!(
                mixed
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'a': [{}, [], '', 0, False, None]}\n"),
                pyo3::ffi::c_str!("test_bindings_json_keyword_mix_expected_empty.py"),
                pyo3::ffi::c_str!("test_bindings_json_keyword_mix_expected_empty"),
            )
            .unwrap();
            assert!(
                empty_mix
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=[{'a': []}, {'b': {}}, '', 0, False, None]\n"),
                pyo3::ffi::c_str!("test_bindings_json_keyword_mix_expected_array.py"),
                pyo3::ffi::c_str!("test_bindings_json_keyword_mix_expected_array"),
            )
            .unwrap();
            assert!(
                array_mix
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
        });
    }

    #[test]
    fn bindings_surface_toml_header_comments_and_crlf_literals() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nquoted_header=t'[\"a.b\"]\\nvalue = 1\\n'\ndotted_quoted_header=t'[site.\"google.com\"]\\nvalue = 1\\n'\ncomment_after_inline_table=Template('value = { a = 1 } # comment\\n')\ncommented_array=t'value = [\\n  1,\\n  # comment\\n  2,\\n]\\n'\nliteral_crlf=Template(\"value = '''a\\r\\nb'''\\n\")\narray_table_followed_by_table=t'[[items]]\\nname = \"a\"\\n\\n[tool]\\nvalue = 1\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_header_comments.py"),
                pyo3::ffi::c_str!("test_bindings_toml_header_comments"),
            )
            .unwrap();

            let quoted_header = render_toml(py, &module.getattr("quoted_header").unwrap()).unwrap();
            let dotted_quoted_header =
                render_toml(py, &module.getattr("dotted_quoted_header").unwrap()).unwrap();
            let comment_after_inline_table =
                render_toml(py, &module.getattr("comment_after_inline_table").unwrap()).unwrap();
            let commented_array =
                render_toml(py, &module.getattr("commented_array").unwrap()).unwrap();
            let literal_crlf = render_toml(py, &module.getattr("literal_crlf").unwrap()).unwrap();
            let array_table_followed_by_table = render_toml(
                py,
                &module.getattr("array_table_followed_by_table").unwrap(),
            )
            .unwrap();

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'a.b': {'value': 1}}\n"),
                pyo3::ffi::c_str!("test_bindings_toml_header_comments_expected_quoted.py"),
                pyo3::ffi::c_str!("test_bindings_toml_header_comments_expected_quoted"),
            )
            .unwrap();
            assert!(
                quoted_header
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'site': {'google.com': {'value': 1}}}\n"),
                pyo3::ffi::c_str!("test_bindings_toml_header_comments_expected_dotted.py"),
                pyo3::ffi::c_str!("test_bindings_toml_header_comments_expected_dotted"),
            )
            .unwrap();
            assert!(
                dotted_quoted_header
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'value': {'a': 1}}\n"),
                pyo3::ffi::c_str!("test_bindings_toml_header_comments_expected_inline.py"),
                pyo3::ffi::c_str!("test_bindings_toml_header_comments_expected_inline"),
            )
            .unwrap();
            assert!(
                comment_after_inline_table
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'value': [1, 2]}\n"),
                pyo3::ffi::c_str!("test_bindings_toml_header_comments_expected_array.py"),
                pyo3::ffi::c_str!("test_bindings_toml_header_comments_expected_array"),
            )
            .unwrap();
            assert!(
                commented_array
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'value': 'a\\nb'}\n"),
                pyo3::ffi::c_str!("test_bindings_toml_header_comments_expected_crlf.py"),
                pyo3::ffi::c_str!("test_bindings_toml_header_comments_expected_crlf"),
            )
            .unwrap();
            assert!(
                literal_crlf
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'items': [{'name': 'a'}], 'tool': {'value': 1}}\n"),
                pyo3::ffi::c_str!("test_bindings_toml_header_comments_expected_table.py"),
                pyo3::ffi::c_str!("test_bindings_toml_header_comments_expected_table"),
            )
            .unwrap();
            assert!(
                array_table_followed_by_table
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
        });
    }

    #[test]
    fn bindings_surface_yaml_comment_streams_and_alias_sequence() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "comment_mid=t'---\\na: 1\\n--- # comment\\n...\\n---\\nb: 2\\n'\ncomment_tail=t'---\\na: 1\\n--- # comment\\n...\\n'\nflow_alias_seq=t'value: [&a 1, *a]\\n'\ndoc_start_comment=t'--- # comment\\nvalue: 1\\n'\ndoc_start_tag_comment=t'--- !!str true # comment\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_comment_streams.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_comment_streams"),
            )
            .unwrap();

            let comment_mid = render_yaml(py, &module.getattr("comment_mid").unwrap()).unwrap();
            let comment_tail = render_yaml(py, &module.getattr("comment_tail").unwrap()).unwrap();
            let flow_alias_seq =
                render_yaml(py, &module.getattr("flow_alias_seq").unwrap()).unwrap();
            let doc_start_comment =
                render_yaml(py, &module.getattr("doc_start_comment").unwrap()).unwrap();
            let doc_start_tag_comment =
                render_yaml(py, &module.getattr("doc_start_tag_comment").unwrap()).unwrap();
            let doc_start_comment_text =
                render_yaml_text(py, &module.getattr("doc_start_comment").unwrap()).unwrap();
            let doc_start_tag_comment_text =
                render_yaml_text(py, &module.getattr("doc_start_tag_comment").unwrap()).unwrap();

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=[{'a': 1}, None, {'b': 2}]\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_comment_streams_expected_mid.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_comment_streams_expected_mid"),
            )
            .unwrap();
            assert!(
                comment_mid
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=[{'a': 1}, None]\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_comment_streams_expected_tail.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_comment_streams_expected_tail"),
            )
            .unwrap();
            assert!(
                comment_tail
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'value': [1, 1]}\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_comment_streams_expected_alias.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_comment_streams_expected_alias"),
            )
            .unwrap();
            assert!(
                flow_alias_seq
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'value': 1}\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_comment_streams_expected_doc.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_comment_streams_expected_doc"),
            )
            .unwrap();
            assert!(
                doc_start_comment
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
            assert_eq!(doc_start_comment_text, "---\nvalue: 1");
            assert_eq!(
                doc_start_tag_comment.bind(py).extract::<String>().unwrap(),
                "true"
            );
            assert_eq!(doc_start_tag_comment_text, "---\n!!str true");

            let comment_mid_text =
                render_yaml_text(py, &module.getattr("comment_mid").unwrap()).unwrap();
            assert_eq!(comment_mid_text, "---\na: 1\n---\nnull\n...\n---\nb: 2");

            let comment_tail_text =
                render_yaml_text(py, &module.getattr("comment_tail").unwrap()).unwrap();
            assert_eq!(comment_tail_text, "---\na: 1\n---\nnull\n...");

            let flow_alias_seq_text =
                render_yaml_text(py, &module.getattr("flow_alias_seq").unwrap()).unwrap();
            assert_eq!(flow_alias_seq_text, "value: [ &a 1, *a ]");
        });
    }

    #[test]
    fn bindings_surface_yaml_root_decorators_and_stream_shapes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "comment_only_explicit_end_document=t'--- # comment\\n...\\n'\ncomment_only_explicit_end_stream=t'--- # comment\\n...\\n---\\na: 1\\n'\nexplicit_end_comment_stream=t'---\\na: 1\\n... # end\\n---\\nb: 2\\n'\nroot_anchor_sequence=t'--- &root\\n  - 1\\n  - 2\\n'\nroot_anchor_custom_mapping=t'--- &root !custom\\n  a: 1\\n'\nroot_custom_anchor_sequence=t'--- !custom &root\\n  - 1\\n  - 2\\n'\ntagged_block_root_mapping=t'--- !!map\\na: 1\\n'\ntagged_block_root_sequence=t'--- !!seq\\n- 1\\n- 2\\n'\nflow_newline=t'{{a: 1, b: [2, 3]}}\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_root_decorators.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_root_decorators"),
            )
            .unwrap();

            let comment_only_explicit_end_document = render_yaml(
                py,
                &module
                    .getattr("comment_only_explicit_end_document")
                    .unwrap(),
            )
            .unwrap();
            assert!(comment_only_explicit_end_document.bind(py).is_none());

            let comment_only_explicit_end_stream = render_yaml(
                py,
                &module.getattr("comment_only_explicit_end_stream").unwrap(),
            )
            .unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=[None, {'a': 1}]\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_root_decorators_expected_stream.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_root_decorators_expected_stream"),
            )
            .unwrap();
            assert!(
                comment_only_explicit_end_stream
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let explicit_end_comment_stream =
                render_yaml(py, &module.getattr("explicit_end_comment_stream").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=[{'a': 1}, {'b': 2}]\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_root_decorators_expected_end_comment.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_root_decorators_expected_end_comment"),
            )
            .unwrap();
            assert!(
                explicit_end_comment_stream
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let root_anchor_sequence =
                render_yaml_text(py, &module.getattr("root_anchor_sequence").unwrap()).unwrap();
            assert_eq!(root_anchor_sequence, "---\n&root\n- 1\n- 2");

            let root_anchor_custom_mapping =
                render_yaml_text(py, &module.getattr("root_anchor_custom_mapping").unwrap())
                    .unwrap();
            assert_eq!(root_anchor_custom_mapping, "---\n!custom &root\na: 1");
            let root_anchor_custom_mapping_data =
                render_yaml(py, &module.getattr("root_anchor_custom_mapping").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'a': 1}\n"),
                pyo3::ffi::c_str!(
                    "test_bindings_yaml_root_decorators_expected_anchor_custom_map.py"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_root_decorators_expected_anchor_custom_map"),
            )
            .unwrap();
            assert!(
                root_anchor_custom_mapping_data
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let root_custom_anchor_sequence =
                render_yaml_text(py, &module.getattr("root_custom_anchor_sequence").unwrap())
                    .unwrap();
            assert_eq!(root_custom_anchor_sequence, "---\n!custom &root\n- 1\n- 2");
            let root_custom_anchor_sequence_data =
                render_yaml(py, &module.getattr("root_custom_anchor_sequence").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=[1, 2]\n"),
                pyo3::ffi::c_str!(
                    "test_bindings_yaml_root_decorators_expected_custom_anchor_seq.py"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_root_decorators_expected_custom_anchor_seq"),
            )
            .unwrap();
            assert!(
                root_custom_anchor_sequence_data
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let tagged_block_root_mapping =
                render_yaml(py, &module.getattr("tagged_block_root_mapping").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'a': 1}\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_root_decorators_expected_map.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_root_decorators_expected_map"),
            )
            .unwrap();
            assert!(
                tagged_block_root_mapping
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
            let tagged_block_root_mapping_text =
                render_yaml_text(py, &module.getattr("tagged_block_root_mapping").unwrap())
                    .unwrap();
            assert_eq!(tagged_block_root_mapping_text, "---\n!!map\na: 1");

            let tagged_block_root_sequence =
                render_yaml(py, &module.getattr("tagged_block_root_sequence").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=[1, 2]\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_root_decorators_expected_seq.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_root_decorators_expected_seq"),
            )
            .unwrap();
            assert!(
                tagged_block_root_sequence
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
            let tagged_block_root_sequence_text =
                render_yaml_text(py, &module.getattr("tagged_block_root_sequence").unwrap())
                    .unwrap();
            assert_eq!(tagged_block_root_sequence_text, "---\n!!seq\n- 1\n- 2");

            let flow_newline =
                render_yaml_text(py, &module.getattr("flow_newline").unwrap()).unwrap();
            assert_eq!(flow_newline, "{ a: 1, b: [ 2, 3 ] }");
        });
    }

    #[test]
    fn bindings_surface_yaml_merge_and_collection_families() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nmerge=t'base: &base\\n  a: 1\\n  b: 2\\nderived:\\n  <<: *base\\n  c: 3\\n'\nflow_nested_alias_merge=Template('value: [{<<: &base {a: 1}, b: 2}, *base]\\n')\nalias_seq_value=t'a: &x [1, 2]\\nb: *x\\n'\nempty_flow_sequence=t'value: []\\n'\nempty_flow_mapping=Template('value: {}\\n')\nflow_mapping_missing_value=Template('value: {a: }\\n')\nindentless_sequence_value=t'a:\\n- 1\\n- 2\\n'\nsequence_of_mappings=t'- a: 1\\n  b: 2\\n- c: 3\\n'\nmapping_of_sequence_of_mappings=t'items:\\n- a: 1\\n  b: 2\\n- c: 3\\n'\nsequence_of_sequences=t'- - 1\\n  - 2\\n- - 3\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_merge_collections.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_merge_collections"),
            )
            .unwrap();

            let merge = render_yaml(py, &module.getattr("merge").unwrap()).unwrap();
            let repr_fn = py.import("builtins").unwrap().getattr("repr").unwrap();
            let merge_repr = repr_fn
                .call1((merge.bind(py),))
                .unwrap()
                .extract::<String>()
                .unwrap();
            assert!(
                merge_repr.contains("'base': {'a': 1, 'b': 2}"),
                "{merge_repr}"
            );
            assert!(merge_repr.contains("'derived':"), "{merge_repr}");
            assert!(merge_repr.contains("'c': 3"), "{merge_repr}");

            let flow_nested_alias_merge =
                render_yaml(py, &module.getattr("flow_nested_alias_merge").unwrap()).unwrap();
            let repr = repr_fn
                .call1((flow_nested_alias_merge.bind(py),))
                .unwrap()
                .extract::<String>()
                .unwrap();
            assert!(repr.contains("'value': ["), "{repr}");

            let alias_seq_value =
                render_yaml(py, &module.getattr("alias_seq_value").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'a': [1, 2], 'b': [1, 2]}\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_merge_collections_expected_alias.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_merge_collections_expected_alias"),
            )
            .unwrap();
            assert!(
                alias_seq_value
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            for (name, code) in [
                ("empty_flow_sequence", "expected={'value': []}\n"),
                ("empty_flow_mapping", "expected={'value': {}}\n"),
                (
                    "flow_mapping_missing_value",
                    "expected={'value': {'a': None}}\n",
                ),
                ("indentless_sequence_value", "expected={'a': [1, 2]}\n"),
                (
                    "sequence_of_mappings",
                    "expected=[{'a': 1, 'b': 2}, {'c': 3}]\n",
                ),
                (
                    "mapping_of_sequence_of_mappings",
                    "expected={'items': [{'a': 1, 'b': 2}, {'c': 3}]}\n",
                ),
                ("sequence_of_sequences", "expected=[[1, 2], [3]]\n"),
            ] {
                let actual = render_yaml(py, &module.getattr(name).unwrap()).unwrap();
                let expected = PyModule::from_code(
                    py,
                    std::ffi::CString::new(code).unwrap().as_c_str(),
                    pyo3::ffi::c_str!("test_bindings_yaml_merge_collections_expected.py"),
                    pyo3::ffi::c_str!("test_bindings_yaml_merge_collections_expected"),
                )
                .unwrap();
                assert!(
                    actual
                        .bind(py)
                        .eq(expected.getattr("expected").unwrap())
                        .unwrap(),
                    "{name}"
                );
            }
        });
    }

    #[test]
    fn bindings_surface_yaml_flow_scalar_edge_families() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nflow_plain_scalar_with_space=t'value: [1 2]\\n'\nmapping_empty_flow_values=Template('value: {a: [], b: {}}\\n')\nflow_mapping_empty_key_and_values=Template('{\"\": [], foo: {}}\\n')\nflow_null_key=Template('{null: 1, \"\": 2}\\n')\nblock_null_key=t'? null\\n: 1\\n'\nquoted_null_key=t'? \"\"\\n: 1\\n'\nplain_question_mark_scalar=t'value: ?x\\n'\nplain_colon_scalar_flow=t'value: [a:b, c:d]\\n'\nflow_mapping_plain_key_questions=Template('value: {?x: 1, ?y: 2}\\n')\nflow_hash_mapping_four=Template('value: {a: b#c, d: e#f, g: h#i, j: k#l}\\n')\nflow_hash_seq_seven=t'value: [a#b, c#d, e#f, g#h, i#j, k#l, m#n]\\n'\ncomment_after_flow_plain_colon=t'value: [a:b # c\\n]\\n'\nflow_plain_comment_after_colon_deeper=t'value: [a:b:c:d # comment\\n]\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_scalar_edges.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_scalar_edges"),
            )
            .unwrap();

            for (name, code) in [
                (
                    "flow_plain_scalar_with_space",
                    "expected={'value': ['1 2']}\n",
                ),
                (
                    "mapping_empty_flow_values",
                    "expected={'value': {'a': [], 'b': {}}}\n",
                ),
                (
                    "flow_mapping_empty_key_and_values",
                    "expected={'': [], 'foo': {}}\n",
                ),
                ("flow_null_key", "expected={None: 1, '': 2}\n"),
                ("block_null_key", "expected={None: 1}\n"),
                ("quoted_null_key", "expected={'': 1}\n"),
                ("plain_question_mark_scalar", "expected={'value': '?x'}\n"),
                (
                    "plain_colon_scalar_flow",
                    "expected={'value': ['a:b', 'c:d']}\n",
                ),
                (
                    "flow_hash_mapping_four",
                    "expected={'value': {'a': 'b#c', 'd': 'e#f', 'g': 'h#i', 'j': 'k#l'}}\n",
                ),
                (
                    "flow_hash_seq_seven",
                    "expected={'value': ['a#b', 'c#d', 'e#f', 'g#h', 'i#j', 'k#l', 'm#n']}\n",
                ),
                (
                    "comment_after_flow_plain_colon",
                    "expected={'value': ['a:b']}\n",
                ),
                (
                    "flow_plain_comment_after_colon_deeper",
                    "expected={'value': ['a:b:c:d']}\n",
                ),
            ] {
                let actual = render_yaml(py, &module.getattr(name).unwrap()).unwrap();
                let expected = PyModule::from_code(
                    py,
                    std::ffi::CString::new(code).unwrap().as_c_str(),
                    pyo3::ffi::c_str!("test_bindings_yaml_flow_scalar_edges_expected.py"),
                    pyo3::ffi::c_str!("test_bindings_yaml_flow_scalar_edges_expected"),
                )
                .unwrap();
                assert!(
                    actual
                        .bind(py)
                        .eq(expected.getattr("expected").unwrap())
                        .unwrap(),
                    "{name}"
                );
            }

            let flow_mapping_plain_key_questions = render_yaml(
                py,
                &module.getattr("flow_mapping_plain_key_questions").unwrap(),
            )
            .unwrap();
            let repr_fn = py.import("builtins").unwrap().getattr("repr").unwrap();
            let repr = repr_fn
                .call1((flow_mapping_plain_key_questions.bind(py),))
                .unwrap()
                .extract::<String>()
                .unwrap();
            assert!(repr.contains("'value':"), "{repr}");
            assert!(repr.contains("1"), "{repr}");
            assert!(repr.contains("2"), "{repr}");
        });
    }

    #[test]
    fn bindings_surface_yaml_flow_collection_comment_and_verbatim_tag_families() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nverbatim_tag=t'value: !<tag:yaml.org,2002:str> hello\\n'\nflow_wrapped_sequence=t'key: [a,\\n  b]\\n'\nflow_wrapped_mapping=Template('key: {a: 1,\\n  b: 2}\\n')\nflow_sequence_comment=t'key: [a, # first\\n  b]\\n'\nflow_mapping_comment=Template('key: {a: 1, # first\\n  b: 2}\\n')\nalias_in_flow_mapping_value=Template('base: &a {x: 1}\\nvalue: {ref: *a}\\n')\nflow_null_and_alias=Template('base: &a {x: 1}\\nvalue: {null: *a}\\n')\n"
                ),
                pyo3::ffi::c_str!(
                    "test_bindings_yaml_flow_collection_comment_and_tag_families.py"
                ),
                pyo3::ffi::c_str!(
                    "test_bindings_yaml_flow_collection_comment_and_tag_families"
                ),
            )
            .unwrap();

            let verbatim_tag = render_yaml(py, &module.getattr("verbatim_tag").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'value': 'hello'}\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_collection_comment_expected_tag.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_collection_comment_expected_tag"),
            )
            .unwrap();
            assert!(
                verbatim_tag
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
            let verbatim_text =
                render_yaml_text(py, &module.getattr("verbatim_tag").unwrap()).unwrap();
            assert_eq!(verbatim_text, "value: !<tag:yaml.org,2002:str> hello");

            for (name, code, expected_text) in [
                (
                    "flow_wrapped_sequence",
                    "expected={'key': ['a', 'b']}\n",
                    "key: [ a, b ]",
                ),
                (
                    "flow_sequence_comment",
                    "expected={'key': ['a', 'b']}\n",
                    "key: [ a, b ]",
                ),
                (
                    "flow_wrapped_mapping",
                    "expected={'key': {'a': 1, 'b': 2}}\n",
                    "key: { a: 1, b: 2 }",
                ),
                (
                    "flow_mapping_comment",
                    "expected={'key': {'a': 1, 'b': 2}}\n",
                    "key: { a: 1, b: 2 }",
                ),
            ] {
                let actual = render_yaml(py, &module.getattr(name).unwrap()).unwrap();
                let expected = PyModule::from_code(
                    py,
                    CString::new(code).unwrap().as_c_str(),
                    pyo3::ffi::c_str!("test_bindings_yaml_flow_collection_comment_expected.py"),
                    pyo3::ffi::c_str!("test_bindings_yaml_flow_collection_comment_expected"),
                )
                .unwrap();
                assert!(
                    actual
                        .bind(py)
                        .eq(expected.getattr("expected").unwrap())
                        .unwrap(),
                    "{name}"
                );
                let rendered = render_yaml_text(py, &module.getattr(name).unwrap()).unwrap();
                assert_eq!(rendered, expected_text, "{name}");
            }

            let alias_in_flow_mapping_value =
                render_yaml(py, &module.getattr("alias_in_flow_mapping_value").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'base': {'x': 1}, 'value': {'ref': {'x': 1}}}\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_collection_comment_expected_alias.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_collection_comment_expected_alias"),
            )
            .unwrap();
            assert!(
                alias_in_flow_mapping_value
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
            let rendered =
                render_yaml_text(py, &module.getattr("alias_in_flow_mapping_value").unwrap())
                    .unwrap();
            assert_eq!(rendered, "base: &a { x: 1 }\nvalue: { ref: *a }");

            let flow_null_and_alias =
                render_yaml(py, &module.getattr("flow_null_and_alias").unwrap()).unwrap();
            let repr_fn = py.import("builtins").unwrap().getattr("repr").unwrap();
            let repr = repr_fn
                .call1((flow_null_and_alias.bind(py),))
                .unwrap()
                .extract::<String>()
                .unwrap();
            assert!(repr.contains("'base': {'x': 1}"), "{repr}");
            assert!(repr.contains("'value': {None: {'x': 1}}"), "{repr}");
            let rendered =
                render_yaml_text(py, &module.getattr("flow_null_and_alias").unwrap()).unwrap();
            assert_eq!(rendered, "base: &a { x: 1 }\nvalue: { null: *a }");
        });
    }

    #[test]
    fn bindings_surface_yaml_verbatim_root_scalar() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("template=t'--- !<tag:yaml.org,2002:str> hello\\n'\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_verbatim_root_scalar.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_verbatim_root_scalar"),
            )
            .unwrap();

            let data = render_yaml(py, &module.getattr("template").unwrap()).unwrap();
            assert_eq!(data.bind(py).extract::<String>().unwrap(), "hello");

            let text = render_yaml_text(py, &module.getattr("template").unwrap()).unwrap();
            assert_eq!(text, "---\n!<tag:yaml.org,2002:str> hello");
        });
    }

    #[test]
    fn bindings_surface_yaml_verbatim_root_anchor_scalar() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("template=t'--- !<tag:yaml.org,2002:str> &root hello\\n'\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_verbatim_root_anchor_scalar.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_verbatim_root_anchor_scalar"),
            )
            .unwrap();

            let data = render_yaml(py, &module.getattr("template").unwrap()).unwrap();
            assert_eq!(data.bind(py).extract::<String>().unwrap(), "hello");

            let text = render_yaml_text(py, &module.getattr("template").unwrap()).unwrap();
            assert_eq!(text, "---\n!<tag:yaml.org,2002:str> &root hello");
        });
    }

    #[test]
    fn bindings_surface_yaml_spec_chapter_2_examples() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "players=t'- Mark McGwire\\n- Sammy Sosa\\n- Ken Griffey\\n'\nclubs=t'american:\\n- Boston Red Sox\\n- Detroit Tigers\\n- New York Yankees\\nnational:\\n- New York Mets\\n- Chicago Cubs\\n- Atlanta Braves\\n'\nstats_seq=t'-\\n  name: Mark McGwire\\n  hr:   65\\n  avg:  0.278\\n-\\n  name: Sammy Sosa\\n  hr:   63\\n  avg:  0.288\\n'\nmap_of_maps=t'Mark McGwire: {{hr: 65, avg: 0.278}}\\nSammy Sosa: {{\\n  hr: 63,\\n  avg: 0.288,\\n}}\\n'\ntwo_docs=t'# Ranking of 1998 home runs\\n---\\n- Mark McGwire\\n- Sammy Sosa\\n- Ken Griffey\\n\\n# Team ranking\\n---\\n- Chicago Cubs\\n- St Louis Cardinals\\n'\nplay_feed=t'---\\ntime: 20:03:20\\nplayer: Sammy Sosa\\naction: strike (miss)\\n...\\n---\\ntime: 20:03:47\\nplayer: Sammy Sosa\\naction: grand slam\\n...\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_spec_chapter_2_examples.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_spec_chapter_2_examples"),
            )
            .unwrap();

            let players = render_yaml(py, &module.getattr("players").unwrap()).unwrap();
            let expected_players = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=['Mark McGwire', 'Sammy Sosa', 'Ken Griffey']\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_players_expected.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_players_expected"),
            )
            .unwrap();
            assert!(
                players
                    .bind(py)
                    .eq(expected_players.getattr("expected").unwrap())
                    .unwrap()
            );
            assert_eq!(
                render_yaml_text(py, &module.getattr("two_docs").unwrap()).unwrap(),
                "---\n- Mark McGwire\n- Sammy Sosa\n- Ken Griffey\n---\n- Chicago Cubs\n- St Louis Cardinals"
            );
            assert_eq!(
                render_yaml_text(py, &module.getattr("play_feed").unwrap()).unwrap(),
                "---\ntime: 20:03:20\nplayer: Sammy Sosa\naction: strike (miss)\n...\n---\ntime: 20:03:47\nplayer: Sammy Sosa\naction: grand slam\n..."
            );
            assert_eq!(
                render_yaml_text(py, &module.getattr("map_of_maps").unwrap()).unwrap(),
                "Mark McGwire: { hr: 65, avg: 0.278 }\nSammy Sosa: { hr: 63, avg: 0.288 }"
            );
        });
    }

    #[test]
    fn bindings_surface_json_unicode_line_and_keyword_arrays() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\narray_with_line_sep=t'[\"\\\\u2028\", \"\\\\u2029\"]'\nupper_unicode_mix_array=Template('[\"\\\\u00DF\", \"\\\\u6771\", \"\\\\u2028\"]')\nkeyword_array=Template('[true,false,null]')\nempty_name_nested_keywords=Template('{\"\": [null, true, false]}')\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_unicode_keyword_arrays.py"),
                pyo3::ffi::c_str!("test_bindings_json_unicode_keyword_arrays"),
            )
            .unwrap();

            let array_with_line_sep =
                render_json(py, &module.getattr("array_with_line_sep").unwrap()).unwrap();
            let upper_unicode_mix_array =
                render_json(py, &module.getattr("upper_unicode_mix_array").unwrap()).unwrap();
            let keyword_array = render_json(py, &module.getattr("keyword_array").unwrap()).unwrap();
            let empty_name_nested_keywords =
                render_json(py, &module.getattr("empty_name_nested_keywords").unwrap()).unwrap();

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=['\\u2028', '\\u2029']\n"),
                pyo3::ffi::c_str!("test_bindings_json_unicode_keyword_arrays_expected_line.py"),
                pyo3::ffi::c_str!("test_bindings_json_unicode_keyword_arrays_expected_line"),
            )
            .unwrap();
            assert!(
                array_with_line_sep
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=['ß', '東', '\\u2028']\n"),
                pyo3::ffi::c_str!("test_bindings_json_unicode_keyword_arrays_expected_upper.py"),
                pyo3::ffi::c_str!("test_bindings_json_unicode_keyword_arrays_expected_upper"),
            )
            .unwrap();
            assert!(
                upper_unicode_mix_array
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=[True, False, None]\n"),
                pyo3::ffi::c_str!("test_bindings_json_unicode_keyword_arrays_expected_kw.py"),
                pyo3::ffi::c_str!("test_bindings_json_unicode_keyword_arrays_expected_kw"),
            )
            .unwrap();
            assert!(
                keyword_array
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'': [None, True, False]}\n"),
                pyo3::ffi::c_str!("test_bindings_json_unicode_keyword_arrays_expected_empty.py"),
                pyo3::ffi::c_str!("test_bindings_json_unicode_keyword_arrays_expected_empty"),
            )
            .unwrap();
            assert!(
                empty_name_nested_keywords
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
        });
    }

    #[test]
    fn bindings_surface_toml_empty_collections_and_empty_paths() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nempty_array=Template('value = []\\n')\nempty_inline_table=Template('value = {}\\n')\nquoted_empty_dotted_table=Template('[a.\"\".b]\\nvalue = 1\\n')\nquoted_empty_subsegments=Template('[\"\".\"\".leaf]\\nvalue = 1\\n')\nquoted_empty_and_named=Template('[\"\".leaf.\"node\"]\\nvalue = 1\\n')\nquoted_empty_leaf_chain=Template('[\"\".\"\".\"leaf\"]\\nvalue = 1\\n')\nmixed_array_tables=Template('[[a]]\\nname = \"x\"\\n[[a]]\\nname = \"y\"\\n')\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_empty_paths.py"),
                pyo3::ffi::c_str!("test_bindings_toml_empty_paths"),
            )
            .unwrap();

            for (attr, expected_src, file_name, module_name) in [
                (
                    "empty_array",
                    "expected={'value': []}\n",
                    "test_bindings_toml_empty_paths_expected_array.py",
                    "test_bindings_toml_empty_paths_expected_array",
                ),
                (
                    "empty_inline_table",
                    "expected={'value': {}}\n",
                    "test_bindings_toml_empty_paths_expected_inline.py",
                    "test_bindings_toml_empty_paths_expected_inline",
                ),
                (
                    "quoted_empty_dotted_table",
                    "expected={'a': {'': {'b': {'value': 1}}}}\n",
                    "test_bindings_toml_empty_paths_expected_dotted.py",
                    "test_bindings_toml_empty_paths_expected_dotted",
                ),
                (
                    "quoted_empty_subsegments",
                    "expected={'': {'': {'leaf': {'value': 1}}}}\n",
                    "test_bindings_toml_empty_paths_expected_subsegments.py",
                    "test_bindings_toml_empty_paths_expected_subsegments",
                ),
                (
                    "quoted_empty_and_named",
                    "expected={'': {'leaf': {'node': {'value': 1}}}}\n",
                    "test_bindings_toml_empty_paths_expected_named.py",
                    "test_bindings_toml_empty_paths_expected_named",
                ),
                (
                    "quoted_empty_leaf_chain",
                    "expected={'': {'': {'leaf': {'value': 1}}}}\n",
                    "test_bindings_toml_empty_paths_expected_leaf.py",
                    "test_bindings_toml_empty_paths_expected_leaf",
                ),
                (
                    "mixed_array_tables",
                    "expected={'a': [{'name': 'x'}, {'name': 'y'}]}\n",
                    "test_bindings_toml_empty_paths_expected_tables.py",
                    "test_bindings_toml_empty_paths_expected_tables",
                ),
            ] {
                let actual = render_toml(py, &module.getattr(attr).unwrap()).unwrap();
                let expected = PyModule::from_code(
                    py,
                    CString::new(expected_src).unwrap().as_c_str(),
                    CString::new(file_name).unwrap().as_c_str(),
                    CString::new(module_name).unwrap().as_c_str(),
                )
                .unwrap();
                assert!(
                    actual
                        .bind(py)
                        .eq(expected.getattr("expected").unwrap())
                        .unwrap()
                );
            }
        });
    }

    #[test]
    fn bindings_surface_yaml_complex_key_render_text() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "left='Alice'\nright='Bob'\ntemplate=t'{{ {{name: [{left}, {right}]}}: 1, [{left}, {right}]: 2 }}'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_complex_key_text.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_complex_key_text"),
            )
            .unwrap();

            let rendered = render_yaml_text(py, &module.getattr("template").unwrap()).unwrap();
            assert_eq!(
                rendered,
                "{ { name: [ \"Alice\", \"Bob\" ] }: 1, [ \"Alice\", \"Bob\" ]: 2 }"
            );

            let data = render_yaml(py, &module.getattr("template").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "expected={frozenset({('name', ('Alice', 'Bob'))}): 1, ('Alice', 'Bob'): 2}\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_complex_key_text_expected.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_complex_key_text_expected"),
            )
            .unwrap();
            assert!(
                data.bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
        });
    }

    #[test]
    fn bindings_surface_json_promoted_fragments_and_row_arrays() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "left='prefix'\nright='suffix'\nrows=[{'name': 'one'}, {'name': 'two'}]\nfragment=t'{{\"label\": {left}-{right}}}'\nvalidated=t'{rows}'\nunvalidated=t'{rows}'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_promoted_fragments.py"),
                pyo3::ffi::c_str!("test_bindings_json_promoted_fragments"),
            )
            .unwrap();

            let fragment = render_json_text(py, &module.getattr("fragment").unwrap()).unwrap();
            assert_eq!(fragment, "{\"label\": \"prefix-suffix\"}");

            let validated = render_json_text(py, &module.getattr("validated").unwrap()).unwrap();
            let unvalidated =
                render_json_text(py, &module.getattr("unvalidated").unwrap()).unwrap();
            assert_eq!(validated, "[{\"name\": \"one\"}, {\"name\": \"two\"}]");
            assert_eq!(unvalidated, "[{\"name\": \"one\"}, {\"name\": \"two\"}]");

            let data = render_json(py, &module.getattr("unvalidated").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=[{'name': 'one'}, {'name': 'two'}]\n"),
                pyo3::ffi::c_str!("test_bindings_json_promoted_fragments_expected.py"),
                pyo3::ffi::c_str!("test_bindings_json_promoted_fragments_expected"),
            )
            .unwrap();
            assert!(
                data.bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
        });
    }

    #[test]
    fn bindings_surface_toml_array_tables_and_comments() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "name='api'\nworker='worker'\narray_tables=t'\\n# comment before content\\n[[services]]\\nname = {name} # inline comment\\n\\n[[services]]\\nname = {worker}\\n'\ncommented_array=t'\\nvalue = [\\n  1,\\n  # comment\\n  2,\\n]\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_array_tables_comments.py"),
                pyo3::ffi::c_str!("test_bindings_toml_array_tables_comments"),
            )
            .unwrap();

            let array_tables = render_toml(py, &module.getattr("array_tables").unwrap()).unwrap();
            let commented_array =
                render_toml(py, &module.getattr("commented_array").unwrap()).unwrap();

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'services': [{'name': 'api'}, {'name': 'worker'}]}\n"),
                pyo3::ffi::c_str!("test_bindings_toml_array_tables_comments_expected.py"),
                pyo3::ffi::c_str!("test_bindings_toml_array_tables_comments_expected"),
            )
            .unwrap();
            assert!(
                array_tables
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'value': [1, 2]}\n"),
                pyo3::ffi::c_str!("test_bindings_toml_array_tables_comments_expected_array.py"),
                pyo3::ffi::c_str!("test_bindings_toml_array_tables_comments_expected_array"),
            )
            .unwrap();
            assert!(
                commented_array
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
        });
    }

    #[test]
    fn bindings_surface_toml_array_of_tables_spec_example() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "template=t'[[products]]\\nname = \"Hammer\"\\nsku = 738594937\\n\\n[[products]]\\nname = \"Nail\"\\nsku = 284758393\\ncolor = \"gray\"\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_array_of_tables_spec_example.py"),
                pyo3::ffi::c_str!("test_bindings_toml_array_of_tables_spec_example"),
            )
            .unwrap();

            let data = render_toml(py, &module.getattr("template").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "expected={'products': [{'name': 'Hammer', 'sku': 738594937}, {'name': 'Nail', 'sku': 284758393, 'color': 'gray'}]}\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_array_of_tables_spec_example_expected.py"),
                pyo3::ffi::c_str!("test_bindings_toml_array_of_tables_spec_example_expected"),
            )
            .unwrap();
            assert!(
                data.bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let text = render_toml_text(py, &module.getattr("template").unwrap()).unwrap();
            assert_eq!(
                text,
                "[[products]]\nname = \"Hammer\"\nsku = 738594937\n[[products]]\nname = \"Nail\"\nsku = 284758393\ncolor = \"gray\""
            );
        });
    }

    #[test]
    fn bindings_surface_toml_nested_array_of_tables_spec_hierarchy() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "template=t'[[fruit]]\\nname = \"apple\"\\n\\n[fruit.physical]\\ncolor = \"red\"\\nshape = \"round\"\\n\\n[[fruit.variety]]\\nname = \"red delicious\"\\n\\n[[fruit.variety]]\\nname = \"granny smith\"\\n\\n[[fruit]]\\nname = \"banana\"\\n\\n[[fruit.variety]]\\nname = \"plantain\"\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_nested_array_tables_spec_example.py"),
                pyo3::ffi::c_str!("test_bindings_toml_nested_array_tables_spec_example"),
            )
            .unwrap();

            let data = render_toml(py, &module.getattr("template").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "expected={'fruit': [{'name': 'apple', 'physical': {'color': 'red', 'shape': 'round'}, 'variety': [{'name': 'red delicious'}, {'name': 'granny smith'}]}, {'name': 'banana', 'variety': [{'name': 'plantain'}]}]}\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_nested_array_tables_spec_example_expected.py"),
                pyo3::ffi::c_str!("test_bindings_toml_nested_array_tables_spec_example_expected"),
            )
            .unwrap();
            assert!(
                data.bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let text = render_toml_text(py, &module.getattr("template").unwrap()).unwrap();
            assert_eq!(
                text,
                "[[fruit]]\nname = \"apple\"\n[fruit.physical]\ncolor = \"red\"\nshape = \"round\"\n[[fruit.variety]]\nname = \"red delicious\"\n[[fruit.variety]]\nname = \"granny smith\"\n[[fruit]]\nname = \"banana\"\n[[fruit.variety]]\nname = \"plantain\""
            );
        });
    }

    #[test]
    fn bindings_surface_toml_main_spec_example() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "template=t'title = \"TOML Example\"\\n\\n[owner]\\nname = \"Tom Preston-Werner\"\\ndob = 1979-05-27T07:32:00-08:00\\n\\n[database]\\nenabled = true\\nports = [ 8000, 8001, 8002 ]\\ndata = [ [\"delta\", \"phi\"], [3.14] ]\\ntemp_targets = {{ cpu = 79.5, case = 72.0 }}\\n\\n[servers]\\n\\n[servers.alpha]\\nip = \"10.0.0.1\"\\nrole = \"frontend\"\\n\\n[servers.beta]\\nip = \"10.0.0.2\"\\nrole = \"backend\"\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_main_spec_example.py"),
                pyo3::ffi::c_str!("test_bindings_toml_main_spec_example"),
            )
            .unwrap();

            let data = render_toml(py, &module.getattr("template").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from datetime import datetime, timedelta, timezone\nexpected={'title': 'TOML Example', 'owner': {'name': 'Tom Preston-Werner', 'dob': datetime(1979, 5, 27, 7, 32, tzinfo=timezone(timedelta(hours=-8)))}, 'database': {'enabled': True, 'ports': [8000, 8001, 8002], 'data': [['delta', 'phi'], [3.14]], 'temp_targets': {'cpu': 79.5, 'case': 72.0}}, 'servers': {'alpha': {'ip': '10.0.0.1', 'role': 'frontend'}, 'beta': {'ip': '10.0.0.2', 'role': 'backend'}}}\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_main_spec_example_expected.py"),
                pyo3::ffi::c_str!("test_bindings_toml_main_spec_example_expected"),
            )
            .unwrap();
            assert!(
                data.bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let text = render_toml_text(py, &module.getattr("template").unwrap()).unwrap();
            assert_eq!(
                text,
                "title = \"TOML Example\"\n[owner]\nname = \"Tom Preston-Werner\"\ndob = 1979-05-27T07:32:00-08:00\n[database]\nenabled = true\nports = [8000, 8001, 8002]\ndata = [[\"delta\", \"phi\"], [3.14]]\ntemp_targets = { cpu = 79.5, case = 72.0 }\n[servers]\n[servers.alpha]\nip = \"10.0.0.1\"\nrole = \"frontend\"\n[servers.beta]\nip = \"10.0.0.2\"\nrole = \"backend\""
            );
        });
    }

    #[test]
    fn bindings_surface_yaml_block_scalar_sequence_text_and_data() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "user='Alice'\ntemplate=t'\\nliteral: |\\n  hello {user}\\n  world\\nfolded: >\\n  hello {user}\\n  world\\nlines:\\n  - |\\n      item {user}\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_block_scalar_sequence.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_block_scalar_sequence"),
            )
            .unwrap();

            let data = render_yaml(py, &module.getattr("template").unwrap()).unwrap();
            let rendered = render_yaml_text(py, &module.getattr("template").unwrap()).unwrap();

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "expected={'literal': 'hello Alice\\nworld\\n', 'folded': 'hello Alice world\\n', 'lines': ['item Alice\\n']}\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_block_scalar_sequence_expected.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_block_scalar_sequence_expected"),
            )
            .unwrap();
            assert!(
                data.bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
            assert_eq!(
                rendered,
                "literal: |\n  hello Alice\n  world\nfolded: >\n  hello Alice\n  world\nlines:\n  - |\n    item Alice\n"
            );
        });
    }

    #[test]
    fn bindings_surface_json_end_to_end_supported_positions() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "key='user'\nleft='prefix'\nright='suffix'\npayload={'enabled': True, 'count': 2}\ntemplate=t'\\n{{\\n  {key}: {payload},\\n  \"prefix-{left}\": \"item-{right}\",\\n  \"label\": {left}-{right}\\n}}\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_e2e_positions.py"),
                pyo3::ffi::c_str!("test_bindings_json_e2e_positions"),
            )
            .unwrap();

            let data = render_json(py, &module.getattr("template").unwrap()).unwrap();
            let text = render_json_text(py, &module.getattr("template").unwrap()).unwrap();

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "expected={'user': {'enabled': True, 'count': 2}, 'prefix-prefix': 'item-suffix', 'label': 'prefix-suffix'}\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_e2e_positions_expected.py"),
                pyo3::ffi::c_str!("test_bindings_json_e2e_positions_expected"),
            )
            .unwrap();
            assert!(
                data.bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
            assert_eq!(
                text,
                "{\"user\": {\"enabled\": true, \"count\": 2}, \"prefix-prefix\": \"item-suffix\", \"label\": \"prefix-suffix\"}"
            );
        });
    }

    #[test]
    fn bindings_surface_json_rfc_8259_image_example() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "template=t'''{{\n  \"Image\": {{\n    \"Width\": 800,\n    \"Height\": 600,\n    \"Title\": \"View from 15th Floor\",\n    \"Thumbnail\": {{\n      \"Url\": \"http://www.example.com/image/481989943\",\n      \"Height\": 125,\n      \"Width\": 100\n    }},\n    \"Animated\": false,\n    \"IDs\": [116, 943, 234, 38793]\n  }}\n}}'''\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_rfc_8259_image_example.py"),
                pyo3::ffi::c_str!("test_bindings_json_rfc_8259_image_example"),
            )
            .unwrap();

            let data = render_json(py, &module.getattr("template").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "expected={'Image': {'Width': 800, 'Height': 600, 'Title': 'View from 15th Floor', 'Thumbnail': {'Url': 'http://www.example.com/image/481989943', 'Height': 125, 'Width': 100}, 'Animated': False, 'IDs': [116, 943, 234, 38793]}}\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_rfc_8259_image_example_expected.py"),
                pyo3::ffi::c_str!("test_bindings_json_rfc_8259_image_example_expected"),
            )
            .unwrap();
            assert!(
                data.bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let text = render_json_text(py, &module.getattr("template").unwrap()).unwrap();
            assert_eq!(
                text,
                "{\"Image\": {\"Width\": 800, \"Height\": 600, \"Title\": \"View from 15th Floor\", \"Thumbnail\": {\"Url\": \"http://www.example.com/image/481989943\", \"Height\": 125, \"Width\": 100}, \"Animated\": false, \"IDs\": [116, 943, 234, 38793]}}"
            );
        });
    }

    #[test]
    fn bindings_surface_json_rfc_8259_value_examples() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "array=t'''[\n  {{\n     \"precision\": \"zip\",\n     \"Latitude\":  37.7668,\n     \"Longitude\": -122.3959,\n     \"Address\":   \"\",\n     \"City\":      \"SAN FRANCISCO\",\n     \"State\":     \"CA\",\n     \"Zip\":       \"94107\",\n     \"Country\":   \"US\"\n  }},\n  {{\n     \"precision\": \"zip\",\n     \"Latitude\":  37.371991,\n     \"Longitude\": -122.026020,\n     \"Address\":   \"\",\n     \"City\":      \"SUNNYVALE\",\n     \"State\":     \"CA\",\n     \"Zip\":       \"94085\",\n     \"Country\":   \"US\"\n  }}\n]'''\nstring=t'\"Hello world!\"'\nnumber=t'42'\nboolean=t'true'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_rfc_8259_value_examples.py"),
                pyo3::ffi::c_str!("test_bindings_json_rfc_8259_value_examples"),
            )
            .unwrap();

            let array = render_json(py, &module.getattr("array").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "expected=[{'precision': 'zip', 'Latitude': 37.7668, 'Longitude': -122.3959, 'Address': '', 'City': 'SAN FRANCISCO', 'State': 'CA', 'Zip': '94107', 'Country': 'US'}, {'precision': 'zip', 'Latitude': 37.371991, 'Longitude': -122.02602, 'Address': '', 'City': 'SUNNYVALE', 'State': 'CA', 'Zip': '94085', 'Country': 'US'}]\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_rfc_8259_value_examples_expected.py"),
                pyo3::ffi::c_str!("test_bindings_json_rfc_8259_value_examples_expected"),
            )
            .unwrap();
            assert!(
                array
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
            assert_eq!(
                render_json_text(py, &module.getattr("array").unwrap()).unwrap(),
                "[{\"precision\": \"zip\", \"Latitude\": 37.7668, \"Longitude\": -122.3959, \"Address\": \"\", \"City\": \"SAN FRANCISCO\", \"State\": \"CA\", \"Zip\": \"94107\", \"Country\": \"US\"}, {\"precision\": \"zip\", \"Latitude\": 37.371991, \"Longitude\": -122.026020, \"Address\": \"\", \"City\": \"SUNNYVALE\", \"State\": \"CA\", \"Zip\": \"94085\", \"Country\": \"US\"}]"
            );
            assert_eq!(
                render_json(py, &module.getattr("string").unwrap())
                    .unwrap()
                    .bind(py)
                    .extract::<String>()
                    .unwrap(),
                "Hello world!"
            );
            assert_eq!(
                render_json_text(py, &module.getattr("number").unwrap()).unwrap(),
                "42"
            );
            assert!(
                render_json(py, &module.getattr("boolean").unwrap())
                    .unwrap()
                    .bind(py)
                    .extract::<bool>()
                    .unwrap()
            );
        });
    }

    #[test]
    fn bindings_surface_toml_end_to_end_supported_positions() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from datetime import UTC, datetime\nkey='leaf'\nleft='prefix'\nright='suffix'\ncreated=datetime(2024, 1, 2, 3, 4, 5, tzinfo=UTC)\ntemplate=t'\\ntitle = \"item-{left}\"\\n[root.{key}]\\nname = {right}\\nlabel = \"{left}-{right}\"\\ncreated = {created}\\nrows = [{left}, {right}]\\nmeta = {{ enabled = true, target = {right} }}\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_e2e_positions.py"),
                pyo3::ffi::c_str!("test_bindings_toml_e2e_positions"),
            )
            .unwrap();

            let data = render_toml(py, &module.getattr("template").unwrap()).unwrap();
            let text = render_toml_text(py, &module.getattr("template").unwrap()).unwrap();

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from datetime import UTC, datetime\nexpected={'title': 'item-prefix', 'root': {'leaf': {'name': 'suffix', 'label': 'prefix-suffix', 'created': datetime(2024, 1, 2, 3, 4, 5, tzinfo=UTC), 'rows': ['prefix', 'suffix'], 'meta': {'enabled': True, 'target': 'suffix'}}}}\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_e2e_positions_expected.py"),
                pyo3::ffi::c_str!("test_bindings_toml_e2e_positions_expected"),
            )
            .unwrap();
            assert!(
                data.bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
            assert_eq!(
                text,
                "title = \"item-prefix\"\n[root.\"leaf\"]\nname = \"suffix\"\nlabel = \"prefix-suffix\"\ncreated = 2024-01-02T03:04:05+00:00\nrows = [\"prefix\", \"suffix\"]\nmeta = { enabled = true, target = \"suffix\" }"
            );
        });
    }

    #[test]
    fn bindings_surface_yaml_end_to_end_supported_positions() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "user='Alice'\nkey='owner'\nanchor='item'\ntag='str'\ntemplate=t'\\n{key}: {user}\\nlabel: \"prefix-{user}\"\\nplain: item-{user}\\nitems:\\n  - &{anchor} {user}\\n  - *{anchor}\\ntagged: !{tag} {user}\\nflow: [{user}, {{label: {user}}}]\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_e2e_positions.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_e2e_positions"),
            )
            .unwrap();

            let data = render_yaml(py, &module.getattr("template").unwrap()).unwrap();
            let text = render_yaml_text(py, &module.getattr("template").unwrap()).unwrap();

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "expected={'owner': 'Alice', 'label': 'prefix-Alice', 'plain': 'item-Alice', 'items': ['Alice', 'Alice'], 'tagged': 'Alice', 'flow': ['Alice', {'label': 'Alice'}]}\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_e2e_positions_expected.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_e2e_positions_expected"),
            )
            .unwrap();
            assert!(
                data.bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
            assert_eq!(
                text,
                "\"owner\": \"Alice\"\nlabel: \"prefix-Alice\"\nplain: item-Alice\nitems:\n  - &item \"Alice\"\n  - *item\ntagged: !str \"Alice\"\nflow: [ \"Alice\", { label: \"Alice\" } ]"
            );
        });
    }

    #[test]
    fn bindings_surface_json_escaped_mix_values() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nescaped_slash_backslash_quote=t'\"\\\\/\\\\\\\\\\\\\"\"'\nescaped_reverse_solidus_solidus=t'\"\\\\\\\\/\"'\nnested_escaped_mix=Template('{\"x\":\"\\\\b\\\\u2028\\\\u2029\\\\/\"}')\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_escaped_mix_values.py"),
                pyo3::ffi::c_str!("test_bindings_json_escaped_mix_values"),
            )
            .unwrap();

            let escaped_slash_backslash_quote = render_json(
                py,
                &module.getattr("escaped_slash_backslash_quote").unwrap(),
            )
            .unwrap();
            let escaped_reverse_solidus_solidus = render_json(
                py,
                &module.getattr("escaped_reverse_solidus_solidus").unwrap(),
            )
            .unwrap();
            let nested_escaped_mix =
                render_json(py, &module.getattr("nested_escaped_mix").unwrap()).unwrap();

            assert_eq!(
                escaped_slash_backslash_quote
                    .bind(py)
                    .extract::<String>()
                    .unwrap(),
                "/\\\""
            );
            assert_eq!(
                escaped_reverse_solidus_solidus
                    .bind(py)
                    .extract::<String>()
                    .unwrap(),
                "\\/"
            );
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'x': '\\b\\u2028\\u2029/'}\n"),
                pyo3::ffi::c_str!("test_bindings_json_escaped_mix_values_expected.py"),
                pyo3::ffi::c_str!("test_bindings_json_escaped_mix_values_expected"),
            )
            .unwrap();
            assert!(
                nested_escaped_mix
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
        });
    }

    #[test]
    fn bindings_surface_toml_quoted_keys_and_multiline_array_comments() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nquoted=t'\"a.b\" = 1\\nsite.\"google.com\".value = 2\\nvalue = [\\n  1, # first\\n  2, # second\\n]\\n'\nempty_basic=Template('\"\" = 1\\n')\nempty_literal=Template(\"'' = 1\\n\")\nempty_segment=Template('a.\"\".b = 1\\n')\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_quoted_keys_comments.py"),
                pyo3::ffi::c_str!("test_bindings_toml_quoted_keys_comments"),
            )
            .unwrap();

            let quoted = render_toml(py, &module.getattr("quoted").unwrap()).unwrap();
            let empty_basic = render_toml(py, &module.getattr("empty_basic").unwrap()).unwrap();
            let empty_literal = render_toml(py, &module.getattr("empty_literal").unwrap()).unwrap();
            let empty_segment = render_toml(py, &module.getattr("empty_segment").unwrap()).unwrap();

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "expected={'a.b': 1, 'site': {'google.com': {'value': 2}}, 'value': [1, 2]}\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_quoted_keys_comments_expected.py"),
                pyo3::ffi::c_str!("test_bindings_toml_quoted_keys_comments_expected"),
            )
            .unwrap();
            assert!(
                quoted
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'': 1}\n"),
                pyo3::ffi::c_str!("test_bindings_toml_quoted_keys_comments_expected_empty.py"),
                pyo3::ffi::c_str!("test_bindings_toml_quoted_keys_comments_expected_empty"),
            )
            .unwrap();
            assert!(
                empty_basic
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
            assert!(
                empty_literal
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'a': {'': {'b': 1}}}\n"),
                pyo3::ffi::c_str!("test_bindings_toml_quoted_keys_comments_expected_segment.py"),
                pyo3::ffi::c_str!("test_bindings_toml_quoted_keys_comments_expected_segment"),
            )
            .unwrap();
            assert!(
                empty_segment
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
        });
    }

    #[test]
    fn bindings_surface_yaml_flow_edge_cases_and_indent_indicator() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nflow_sequence=t'[1, 2,]\\n'\nflow_mapping=Template('{a: 1,}\\n')\nexplicit_key_sequence_value=t'? a\\n: - 1\\n  - 2\\n'\nindent_indicator=t'value: |1\\n a\\n b\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_indent.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_indent"),
            )
            .unwrap();

            let flow_sequence = render_yaml(py, &module.getattr("flow_sequence").unwrap()).unwrap();
            let flow_mapping = render_yaml(py, &module.getattr("flow_mapping").unwrap()).unwrap();
            let explicit_key_sequence_value =
                render_yaml(py, &module.getattr("explicit_key_sequence_value").unwrap()).unwrap();
            let indent_indicator =
                render_yaml(py, &module.getattr("indent_indicator").unwrap()).unwrap();
            let indent_indicator_text =
                render_yaml_text(py, &module.getattr("indent_indicator").unwrap()).unwrap();

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=[1, 2]\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_indent_expected_seq.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_indent_expected_seq"),
            )
            .unwrap();
            assert!(
                flow_sequence
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'a': 1}\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_indent_expected_map.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_indent_expected_map"),
            )
            .unwrap();
            assert!(
                flow_mapping
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'a': [1, 2]}\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_indent_expected_key.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_indent_expected_key"),
            )
            .unwrap();
            assert!(
                explicit_key_sequence_value
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'value': 'a\\nb\\n'}\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_indent_expected_indent.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_flow_indent_expected_indent"),
            )
            .unwrap();
            assert!(
                indent_indicator
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
            assert_eq!(indent_indicator_text, "value: |1\n a\n b\n");
        });
    }

    #[test]
    fn bindings_surface_json_nested_nulls_and_number_whitespace() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nnested_nulls=Template('{\"a\": null, \"b\": [null, {\"c\": null}]}')\nnested_number_whitespace=Template('{\"a\": [ 0 , -0 , 1.5E-2 ] }')\ntop_ws_string=Template('\\n\\r\\t \"x\" \\n')\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_nulls_numbers.py"),
                pyo3::ffi::c_str!("test_bindings_json_nulls_numbers"),
            )
            .unwrap();

            let nested_nulls = render_json(py, &module.getattr("nested_nulls").unwrap()).unwrap();
            let nested_number_whitespace =
                render_json(py, &module.getattr("nested_number_whitespace").unwrap()).unwrap();
            let top_ws_string = render_json(py, &module.getattr("top_ws_string").unwrap()).unwrap();

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'a': None, 'b': [None, {'c': None}]}\n"),
                pyo3::ffi::c_str!("test_bindings_json_nulls_numbers_expected_nulls.py"),
                pyo3::ffi::c_str!("test_bindings_json_nulls_numbers_expected_nulls"),
            )
            .unwrap();
            assert!(
                nested_nulls
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'a': [0, 0, 0.015]}\n"),
                pyo3::ffi::c_str!("test_bindings_json_nulls_numbers_expected_numbers.py"),
                pyo3::ffi::c_str!("test_bindings_json_nulls_numbers_expected_numbers"),
            )
            .unwrap();
            assert!(
                nested_number_whitespace
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            assert_eq!(top_ws_string.bind(py).extract::<String>().unwrap(), "x");
        });
    }

    #[test]
    fn bindings_surface_toml_empty_strings_and_quoted_empty_headers() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nbasic=t'value = \"\"\\n'\nliteral=Template(\"value = ''\\n\")\nheader=Template('[\"\"]\\nvalue = 1\\n')\nheader_subtable=Template('[\"\"]\\nvalue = 1\\n[\"\".inner]\\nname = \"x\"\\n')\nquoted_header_segments=Template('[\"a\".\"b\"]\\nvalue = 1\\n')\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_empty_headers.py"),
                pyo3::ffi::c_str!("test_bindings_toml_empty_headers"),
            )
            .unwrap();

            let basic = render_toml(py, &module.getattr("basic").unwrap()).unwrap();
            let literal = render_toml(py, &module.getattr("literal").unwrap()).unwrap();
            let header = render_toml(py, &module.getattr("header").unwrap()).unwrap();
            let header_subtable =
                render_toml(py, &module.getattr("header_subtable").unwrap()).unwrap();
            let quoted_header_segments =
                render_toml(py, &module.getattr("quoted_header_segments").unwrap()).unwrap();

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'value': ''}\n"),
                pyo3::ffi::c_str!("test_bindings_toml_empty_headers_expected_empty.py"),
                pyo3::ffi::c_str!("test_bindings_toml_empty_headers_expected_empty"),
            )
            .unwrap();
            assert!(
                basic
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
            assert!(
                literal
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'': {'value': 1}}\n"),
                pyo3::ffi::c_str!("test_bindings_toml_empty_headers_expected_header.py"),
                pyo3::ffi::c_str!("test_bindings_toml_empty_headers_expected_header"),
            )
            .unwrap();
            assert!(
                header
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'': {'value': 1, 'inner': {'name': 'x'}}}\n"),
                pyo3::ffi::c_str!("test_bindings_toml_empty_headers_expected_subtable.py"),
                pyo3::ffi::c_str!("test_bindings_toml_empty_headers_expected_subtable"),
            )
            .unwrap();
            assert!(
                header_subtable
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'a': {'b': {'value': 1}}}\n"),
                pyo3::ffi::c_str!("test_bindings_toml_empty_headers_expected_segments.py"),
                pyo3::ffi::c_str!("test_bindings_toml_empty_headers_expected_segments"),
            )
            .unwrap();
            assert!(
                quoted_header_segments
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
        });
    }

    #[test]
    fn bindings_surface_yaml_scalar_semantics_and_top_level_sequence() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "user='Alice'\nmapping=t'\\non: on\\nyes: yes\\ntruth: true\\nempty: null\\n'\nsequence=t'\\n- {user}\\n- true\\n- on\\n'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_scalar_semantics.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_scalar_semantics"),
            )
            .unwrap();

            let mapping = render_yaml(py, &module.getattr("mapping").unwrap()).unwrap();
            let sequence = render_yaml(py, &module.getattr("sequence").unwrap()).unwrap();

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "expected={'on': 'on', 'yes': 'yes', 'truth': True, 'empty': None}\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_scalar_semantics_expected_map.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_scalar_semantics_expected_map"),
            )
            .unwrap();
            assert!(
                mapping
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=['Alice', True, 'on']\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_scalar_semantics_expected_seq.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_scalar_semantics_expected_seq"),
            )
            .unwrap();
            assert!(
                sequence
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
        });
    }

    #[test]
    fn bindings_surface_json_structural_invalid_message_families() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nmissing_comma=Template('[\"a\" true]')\ntrailing_comma=Template('{\"a\":1,}')\ninvalid_fragment=Template('[null {\"a\":1}]')\nunexpected_trailing=Template('{\"a\":1}}')\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_structural_invalids.py"),
                pyo3::ffi::c_str!("test_bindings_json_structural_invalids"),
            )
            .unwrap();

            let missing_comma = module.getattr("missing_comma").unwrap();
            let err = render_json_text(py, &missing_comma)
                .expect_err("expected JSON missing-comma parse error");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(err.to_string().contains("Expected ',' in JSON template"));

            let trailing_comma = module.getattr("trailing_comma").unwrap();
            let err = render_json_text(py, &trailing_comma)
                .expect_err("expected JSON trailing-comma parse error");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(err.to_string().contains("quoted strings or interpolations"));

            let invalid_fragment = module.getattr("invalid_fragment").unwrap();
            let err = render_json_text(py, &invalid_fragment)
                .expect_err("expected invalid promoted JSON fragment parse error");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(
                err.to_string()
                    .contains("Invalid promoted JSON fragment content")
            );

            let unexpected_trailing = module.getattr("unexpected_trailing").unwrap();
            let err = render_json_text(py, &unexpected_trailing)
                .expect_err("expected JSON trailing-content parse error");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(
                err.to_string()
                    .contains("Unexpected trailing content in JSON template")
            );
        });
    }

    #[test]
    fn bindings_surface_json_additional_parse_error_families() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nleading_zero_exp=Template('01e0')\nleading_zero_exp_negative=Template('-01e0')\nmissing_exp_digits=Template('1e-')\nembedded_missing_exp_digits=Template('{\"x\": 1e-}')\ndouble_sign_number=Template('-+1')\nleading_plus_minus=Template('+-1')\nbad_exp_plus_minus=Template('1e+-1')\nbad_exp_minus_plus=Template('1e-+1')\nextra_decimal=Template('1.2.3')\narray_space_number=Template('[1 2]')\nobject_space_number=Template('{\"a\":1 \"b\":2}')\ntruee=Template('[truee]')\ntrue_then_number=Template('[true 1]')\nobject_true_then_number=Template('{\"a\": true 1}')\nfalse_fragment=Template('[false {\"a\":1}]')\narray_trailing_object=Template('{\"a\": [1]}]')\nobject_trailing_array=Template('[{\"a\":1}]]')\ndeep_object_trailing=Template('{\"a\": {\"b\": 1}}}')\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_additional_parse_errors.py"),
                pyo3::ffi::c_str!("test_bindings_json_additional_parse_errors"),
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
                let err =
                    render_json_text(py, &template).expect_err("expected JSON parse family error");
                assert!(err.is_instance_of::<TemplateParseError>(py));
                assert!(err.to_string().contains(expected), "{name}: {err}");
            }
        });
    }

    #[test]
    fn bindings_surface_render_tomlrailing_arrays_and_validation_false() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nvalue='Alice'\ntrailing_ints=Template('value = [1, 2,]\\n')\ntrailing_dates=Template('value = [2024-01-02, 2024-01-03,]\\n')\nvalidated=t'name = {value}'\ninvalid=Template('value = [1,,2]\\n')\n"
                ),
                pyo3::ffi::c_str!("test_bindings_render_tomlrailing_arrays.py"),
                pyo3::ffi::c_str!("test_bindings_render_tomlrailing_arrays"),
            )
            .unwrap();

            let trailing_ints = render_toml(py, &module.getattr("trailing_ints").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'value': [1, 2]}\n"),
                pyo3::ffi::c_str!("test_bindings_render_tomlrailing_arrays_expected_ints.py"),
                pyo3::ffi::c_str!("test_bindings_render_tomlrailing_arrays_expected_ints"),
            )
            .unwrap();
            assert!(
                trailing_ints
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let trailing_dates =
                render_toml(py, &module.getattr("trailing_dates").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from datetime import date\nexpected={'value': [date(2024, 1, 2), date(2024, 1, 3)]}\n"
                ),
                pyo3::ffi::c_str!("test_bindings_render_tomlrailing_arrays_expected_dates.py"),
                pyo3::ffi::c_str!("test_bindings_render_tomlrailing_arrays_expected_dates"),
            )
            .unwrap();
            assert!(
                trailing_dates
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let validated = module.getattr("validated").unwrap();
            let validated_text = render_toml_text(py, &validated).unwrap();
            let unvalidated_text = render_toml_text(py, &validated).unwrap();
            assert_eq!(validated_text, "name = \"Alice\"");
            assert_eq!(validated_text, unvalidated_text);

            let invalid = module.getattr("invalid").unwrap();
            let err = render_toml(py, &invalid)
                .expect_err("expected TOML parse error with validation disabled");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(err.to_string().contains("Expected a TOML value"));
        });
    }

    #[test]
    fn bindings_surface_toml_additional_invalid_literal_families() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ninvalid_exp_mixed_sign=Template('value = 1e_+1\\n')\ninvalid_float_then_exp=Template('value = 1.e1\\n')\ninvalid_inline_pos=Template('value = { pos = ++1 }\\n')\ninvalid_nested_inline_pos=Template('value = { inner = { pos = ++1 } }\\n')\ninvalid_triple_nested_plus=Template('value = [[[++1]]]\\n')\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_additional_invalid_literals.py"),
                pyo3::ffi::c_str!("test_bindings_toml_additional_invalid_literals"),
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
                let err = render_toml(py, &template).expect_err("expected TOML parse family error");
                assert!(err.is_instance_of::<TemplateParseError>(py));
                assert!(
                    err.to_string().contains("Invalid TOML literal"),
                    "{name}: {err}"
                );
            }
        });
    }

    #[test]
    fn bindings_surface_yaml_empty_documents_and_complex_key_shapes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "empty_document=t'--- # comment\\n'\nempty_document_stream=t'---\\n\\n---\\na: 1\\n'\ncomment_only_tail_stream=t'---\\na: 1\\n--- # comment\\n...\\n'\ncomplex_key=t'{{ {{name: [\"Alice\", \"Bob\"]}}: 1, [\"Alice\", \"Bob\"]: 2 }}'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_empty_documents.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_empty_documents"),
            )
            .unwrap();

            let empty_document =
                render_yaml(py, &module.getattr("empty_document").unwrap()).unwrap();
            assert!(empty_document.bind(py).is_none());

            let empty_document_stream =
                render_yaml(py, &module.getattr("empty_document_stream").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=[None, {'a': 1}]\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_empty_documents_expected_stream.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_empty_documents_expected_stream"),
            )
            .unwrap();
            assert!(
                empty_document_stream
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let comment_only_tail_stream =
                render_yaml(py, &module.getattr("comment_only_tail_stream").unwrap()).unwrap();
            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=[{'a': 1}, None]\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_empty_documents_expected_tail.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_empty_documents_expected_tail"),
            )
            .unwrap();
            assert!(
                comment_only_tail_stream
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let complex_key = render_yaml(py, &module.getattr("complex_key").unwrap()).unwrap();
            let repr_fn = py.import("builtins").unwrap().getattr("repr").unwrap();
            let complex_repr = repr_fn
                .call1((complex_key.bind(py),))
                .unwrap()
                .extract::<String>()
                .unwrap();
            assert_eq!(
                complex_repr,
                "{frozenset({('name', ('Alice', 'Bob'))}): 1, ('Alice', 'Bob'): 2}"
            );
        });
    }

    #[test]
    fn bindings_surface_json_validation_false_and_unrepresentable_contracts() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "name='Alice'\nbad_key=3\nbad_mapping={1: 'x'}\nbad_value={1, 2}\nvalidated=t'{{\"name\": {name}}}'\ninvalid=t'{{name: 1}}'\nkey_text=t'{{{bad_key}: 1}}'\nkey_data=t'{{\"payload\": {bad_mapping}}}'\nset_value=t'{{\"items\": {bad_value}}}'\nfloat_value=t'{{\"ratio\": {float(\"inf\")}}}'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_validation_unrepr.py"),
                pyo3::ffi::c_str!("test_bindings_json_validation_unrepr"),
            )
            .unwrap();

            let validated = module.getattr("validated").unwrap();
            let validated_text = render_json_text(py, &validated).unwrap();
            let unvalidated_text = render_json_text(py, &validated).unwrap();
            let data = render_json(py, &validated).unwrap();
            assert_eq!(validated_text, "{\"name\": \"Alice\"}");
            assert_eq!(validated_text, unvalidated_text);

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'name': 'Alice'}\n"),
                pyo3::ffi::c_str!("test_bindings_json_validation_unrepr_expected.py"),
                pyo3::ffi::c_str!("test_bindings_json_validation_unrepr_expected"),
            )
            .unwrap();
            assert!(
                data.bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let invalid = module.getattr("invalid").unwrap();
            let err = render_json_text(py, &invalid)
                .expect_err("expected JSON parse error with validation disabled");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(err.to_string().contains("JSON object keys"));

            let key_text = module.getattr("key_text").unwrap();
            let err =
                render_json_text(py, &key_text).expect_err("expected JSON object-key render error");
            assert!(err.is_instance_of::<UnrepresentableValueError>(py));
            assert!(err.to_string().contains("object key"));

            let key_data = module.getattr("key_data").unwrap();
            let err = render_json(py, &key_data).expect_err("expected JSON key-data error");
            assert!(err.is_instance_of::<UnrepresentableValueError>(py));
            assert!(err.to_string().contains("object key"));

            let set_value = module.getattr("set_value").unwrap();
            let err = render_json_text(py, &set_value).expect_err("expected JSON set render error");
            assert!(err.is_instance_of::<UnrepresentableValueError>(py));
            assert!(err.to_string().contains("set"));

            let float_value = module.getattr("float_value").unwrap();
            let err = render_json_text(py, &float_value)
                .expect_err("expected JSON non-finite float render error");
            assert!(err.is_instance_of::<UnrepresentableValueError>(py));
            assert!(err.to_string().contains("non-finite float"));
        });
    }

    #[test]
    fn bindings_surface_toml_value_and_mode_contracts() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from datetime import UTC, time\nfrom string.templatelib import Template\nclass BadStringValue:\n    def __str__(self):\n        raise ValueError('cannot stringify')\nvalue='Alice'\nbad_key=3\nbad_time=time(1, 2, 3, tzinfo=UTC)\nbad_fragment=BadStringValue()\nvalidated=t'name = {value}'\ninvalid=t'name = '\nkey_template=t'{bad_key} = 1'\nnull_template=t'name = {None}'\ntime_template=t'when = {bad_time}'\nfragment_template=t'title = \"hi-{bad_fragment}\"'\nduplicate_table=Template('[a]\\nvalue = 1\\n[a]\\nname = \"x\"\\n')\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_value_final.py"),
                pyo3::ffi::c_str!("test_bindings_toml_value_final"),
            )
            .unwrap();

            let validated = module.getattr("validated").unwrap();
            let validated_text = render_toml_text(py, &validated).unwrap();
            let unvalidated_text = render_toml_text(py, &validated).unwrap();
            let data = render_toml(py, &validated).unwrap();
            assert_eq!(validated_text, "name = \"Alice\"");
            assert_eq!(validated_text, unvalidated_text);

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'name': 'Alice'}\n"),
                pyo3::ffi::c_str!("test_bindings_toml_value_final_expected.py"),
                pyo3::ffi::c_str!("test_bindings_toml_value_final_expected"),
            )
            .unwrap();
            assert!(
                data.bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let invalid = module.getattr("invalid").unwrap();
            let err = render_toml_text(py, &invalid)
                .expect_err("expected TOML parse error with validation disabled");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(err.to_string().contains("Expected a TOML value"));

            let key_template = module.getattr("key_template").unwrap();
            let err = render_toml_text(py, &key_template).expect_err("expected TOML key error");
            assert!(err.is_instance_of::<UnrepresentableValueError>(py));
            assert!(err.to_string().contains("TOML key"));

            let null_template = module.getattr("null_template").unwrap();
            let err = render_toml_text(py, &null_template).expect_err("expected TOML null error");
            assert!(err.is_instance_of::<UnrepresentableValueError>(py));
            assert!(err.to_string().contains("no null value"));

            let time_template = module.getattr("time_template").unwrap();
            let err =
                render_toml_text(py, &time_template).expect_err("expected TOML timezone error");
            assert!(err.is_instance_of::<UnrepresentableValueError>(py));
            assert!(err.to_string().contains("timezone"));

            let fragment_template = module.getattr("fragment_template").unwrap();
            let err = render_toml_text(py, &fragment_template)
                .expect_err("expected TOML fragment stringification error");
            assert!(err.is_instance_of::<UnrepresentableValueError>(py));
            assert!(err.to_string().contains("string fragment"));

            let duplicate_table = module.getattr("duplicate_table").unwrap();
            let err = render_toml(py, &duplicate_table)
                .expect_err("expected TOML duplicate-key semantic error");
            assert!(err.is_instance_of::<TemplateSemanticError>(py));
            assert!(err.to_string().to_lowercase().contains("duplicate"));
        });
    }

    #[test]
    fn bindings_surface_json_escape_unicode_and_keyword_text_shapes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ntop_bool_ws=Template(' \\n true \\t ')\ntop_null_ws=Template(' \\r\\n null \\n')\narray_with_line_sep=t'[\"\\\\u2028\", \"\\\\u2029\"]'\nunicode_mix_nested_obj=Template('{\"x\": {\"a\": \"\\\\u005C\", \"b\": \"\\\\u00DF\", \"c\": \"\\\\u2029\"}}')\nkeyword_array=Template('[true,false,null]')\n"
                ),
                pyo3::ffi::c_str!("test_bindings_json_escape_unicode_keyword.py"),
                pyo3::ffi::c_str!("test_bindings_json_escape_unicode_keyword"),
            )
            .unwrap();

            let top_bool_ws_data =
                render_json(py, &module.getattr("top_bool_ws").unwrap()).unwrap();
            let top_bool_ws_text =
                render_json_text(py, &module.getattr("top_bool_ws").unwrap()).unwrap();
            let top_null_ws_data =
                render_json(py, &module.getattr("top_null_ws").unwrap()).unwrap();
            let top_null_ws_text =
                render_json_text(py, &module.getattr("top_null_ws").unwrap()).unwrap();
            let array_with_line_sep =
                render_json(py, &module.getattr("array_with_line_sep").unwrap()).unwrap();
            let array_with_line_sep_text =
                render_json_text(py, &module.getattr("array_with_line_sep").unwrap()).unwrap();
            let unicode_mix_nested_obj =
                render_json(py, &module.getattr("unicode_mix_nested_obj").unwrap()).unwrap();
            let unicode_mix_nested_obj_text =
                render_json_text(py, &module.getattr("unicode_mix_nested_obj").unwrap()).unwrap();
            let keyword_array = render_json(py, &module.getattr("keyword_array").unwrap()).unwrap();
            let keyword_array_text =
                render_json_text(py, &module.getattr("keyword_array").unwrap()).unwrap();

            assert!(top_bool_ws_data.bind(py).is_truthy().unwrap());
            assert_eq!(top_bool_ws_text, "true");
            assert!(top_null_ws_data.bind(py).is_none());
            assert_eq!(top_null_ws_text, "null");

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=['\\u2028', '\\u2029']\n"),
                pyo3::ffi::c_str!("test_bindings_json_escape_unicode_keyword_expected_array.py"),
                pyo3::ffi::c_str!("test_bindings_json_escape_unicode_keyword_expected_array"),
            )
            .unwrap();
            assert!(
                array_with_line_sep
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
            assert_eq!(array_with_line_sep_text, "[\"\u{2028}\", \"\u{2029}\"]");

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'x': {'a': '\\\\', 'b': 'ß', 'c': '\\u2029'}}\n"),
                pyo3::ffi::c_str!("test_bindings_json_escape_unicode_keyword_expected_obj.py"),
                pyo3::ffi::c_str!("test_bindings_json_escape_unicode_keyword_expected_obj"),
            )
            .unwrap();
            assert!(
                unicode_mix_nested_obj
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
            assert_eq!(
                unicode_mix_nested_obj_text,
                "{\"x\": {\"a\": \"\\\\\", \"b\": \"ß\", \"c\": \"\u{2029}\"}}"
            );

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected=[True, False, None]\n"),
                pyo3::ffi::c_str!("test_bindings_json_escape_unicode_keyword_expected_keywords.py"),
                pyo3::ffi::c_str!("test_bindings_json_escape_unicode_keyword_expected_keywords"),
            )
            .unwrap();
            assert!(
                keyword_array
                    .bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
            assert_eq!(keyword_array_text, "[true, false, null]");
        });
    }

    #[test]
    fn bindings_surface_toml_numeric_and_datetime_literal_shapes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\ntemplate=Template('plus_int = +1\\nplus_zero = +0\\nplus_zero_float = +0.0\\nlocal_date = 2024-01-02\\nlocal_time_fraction = 03:04:05.123456\\noffset_fraction_dt = 1979-05-27T07:32:00.999999-07:00\\nutc_fraction_lower_array = [2024-01-02T03:04:05.123456z, 2024-01-02T03:04:06z]\\nsigned_int_array = [+1, +0, -1]\\n')\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_numeric_datetime_literal.py"),
                pyo3::ffi::c_str!("test_bindings_toml_numeric_datetime_literal"),
            )
            .unwrap();

            let data = render_toml(py, &module.getattr("template").unwrap()).unwrap();
            let text = render_toml_text(py, &module.getattr("template").unwrap()).unwrap();

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from datetime import UTC, date, datetime, time, timedelta, timezone\nexpected={'plus_int': 1, 'plus_zero': 0, 'plus_zero_float': 0.0, 'local_date': date(2024, 1, 2), 'local_time_fraction': time(3, 4, 5, 123456), 'offset_fraction_dt': datetime(1979, 5, 27, 7, 32, 0, 999999, tzinfo=timezone(timedelta(hours=-7))), 'utc_fraction_lower_array': [datetime(2024, 1, 2, 3, 4, 5, 123456, tzinfo=UTC), datetime(2024, 1, 2, 3, 4, 6, tzinfo=UTC)], 'signed_int_array': [1, 0, -1]}\n"
                ),
                pyo3::ffi::c_str!("test_bindings_toml_numeric_datetime_literal_expected.py"),
                pyo3::ffi::c_str!("test_bindings_toml_numeric_datetime_literal_expected"),
            )
            .unwrap();
            assert!(
                data.bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );
            assert_eq!(
                text,
                "plus_int = +1\nplus_zero = +0\nplus_zero_float = +0.0\nlocal_date = 2024-01-02\nlocal_time_fraction = 03:04:05.123456\noffset_fraction_dt = 1979-05-27T07:32:00.999999-07:00\nutc_fraction_lower_array = [2024-01-02T03:04:05.123456z, 2024-01-02T03:04:06z]\nsigned_int_array = [+1, +0, -1]"
            );
        });
    }

    #[test]
    fn bindings_surface_yaml_error_and_validation_false_contracts() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nclass BadStringValue:\n    def __str__(self):\n        raise ValueError('cannot stringify')\nvalue='Alice'\nbad=BadStringValue()\ntag='bad tag'\nvalidated=t'name: {value}'\nparse_open=t'value: [1, 2'\nparse_tab=Template('a:\\t1\\n')\nparse_nested_tab=Template('a:\\n  b:\\n\\t- 1\\n')\nparse_trailing=Template('value: *not alias\\n')\nparse_empty_flow=Template('[1,,2]\\n')\nparse_trailing_entry=Template('value: [1, 2,,]\\n')\nparse_empty_mapping=Template('value: {,}\\n')\nparse_missing_colon=Template('value: {a b}\\n')\nparse_extra_comma=Template('value: {a: 1,, b: 2}\\n')\nunknown_anchor=Template('value: *not_alias\\n')\nduplicate_anchor=Template('first: &a 1\\nsecond: &a 2\\nref: *a\\n')\ncross_doc_anchor=Template('--- &a\\n- 1\\n- 2\\n---\\n*a\\n')\nfragment_template=t'label: \"hi-{bad}\"'\nmetadata_template=t'value: !{tag} ok'\nfloat_template=t'value: {float(\"inf\")}'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_error_validation.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_error_validation"),
            )
            .unwrap();

            let validated = module.getattr("validated").unwrap();
            let validated_text = render_yaml_text(py, &validated).unwrap();
            let unvalidated_text = render_yaml_text(py, &validated).unwrap();
            let data = render_yaml(py, &validated).unwrap();
            assert_eq!(validated_text, "name: \"Alice\"");
            assert_eq!(validated_text, unvalidated_text);

            let expected = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("expected={'name': 'Alice'}\n"),
                pyo3::ffi::c_str!("test_bindings_yaml_error_validation_expected.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_error_validation_expected"),
            )
            .unwrap();
            assert!(
                data.bind(py)
                    .eq(expected.getattr("expected").unwrap())
                    .unwrap()
            );

            let parse_open = module.getattr("parse_open").unwrap();
            let err = render_yaml_text(py, &parse_open)
                .expect_err("expected YAML parse error with validation disabled");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(err.to_string().contains("Expected"));

            let parse_tab = module.getattr("parse_tab").unwrap();
            let err = render_yaml_text(py, &parse_tab).expect_err("expected YAML tab parse error");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(err.to_string().contains("Tabs are not allowed"));

            let parse_nested_tab = module.getattr("parse_nested_tab").unwrap();
            let err = render_yaml_text(py, &parse_nested_tab)
                .expect_err("expected YAML nested-tab parse error");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(err.to_string().contains("Tabs are not allowed"));

            let parse_trailing = module.getattr("parse_trailing").unwrap();
            let err = render_yaml_text(py, &parse_trailing)
                .expect_err("expected YAML trailing parse error");
            assert!(err.is_instance_of::<TemplateParseError>(py));
            assert!(err.to_string().contains("Unexpected trailing YAML content"));

            for (name, expected) in [
                ("parse_empty_flow", "Expected a YAML value"),
                ("parse_trailing_entry", "Expected"),
                ("parse_empty_mapping", "Expected ':' in YAML template"),
                ("parse_missing_colon", "Expected ':' in YAML template"),
                ("parse_extra_comma", "Expected ':' in YAML template"),
            ] {
                let template = module.getattr(name).unwrap();
                let err =
                    render_yaml_text(py, &template).expect_err("expected YAML parse family error");
                assert!(err.is_instance_of::<TemplateParseError>(py));
                assert!(err.to_string().contains(expected), "{name}: {err}");
            }

            let unknown_anchor = module.getattr("unknown_anchor").unwrap();
            let err = render_yaml_text(py, &unknown_anchor)
                .expect_err("expected YAML unknown-anchor semantic error");
            assert!(err.is_instance_of::<TemplateSemanticError>(py));
            assert!(err.to_string().contains("unknown anchor"));

            let duplicate_anchor = module.getattr("duplicate_anchor").unwrap();
            let text = render_yaml_text(py, &duplicate_anchor)
                .expect("expected YAML duplicate-anchor text rendering to succeed");
            assert_eq!(text, "first: &a 1\nsecond: &a 2\nref: *a");

            let cross_doc_anchor = module.getattr("cross_doc_anchor").unwrap();
            let err = render_yaml_text(py, &cross_doc_anchor)
                .expect_err("expected YAML cross-document anchor semantic error");
            assert!(err.is_instance_of::<TemplateSemanticError>(py));
            assert!(err.to_string().contains("unknown anchor"));

            let fragment_template = module.getattr("fragment_template").unwrap();
            let err =
                render_yaml_text(py, &fragment_template).expect_err("expected YAML fragment error");
            assert!(err.is_instance_of::<UnrepresentableValueError>(py));
            assert!(err.to_string().contains("fragment"));

            let metadata_template = module.getattr("metadata_template").unwrap();
            let err =
                render_yaml_text(py, &metadata_template).expect_err("expected YAML metadata error");
            assert!(err.is_instance_of::<UnrepresentableValueError>(py));
            assert!(err.to_string().contains("metadata"));

            let float_template = module.getattr("float_template").unwrap();
            let err = render_yaml_text(py, &float_template)
                .expect_err("expected YAML non-finite float error");
            assert!(err.is_instance_of::<UnrepresentableValueError>(py));
            assert!(err.to_string().contains("non-finite float"));
        });
    }

    #[test]
    fn yaml_static_data_path_uses_rust_backend_semantics() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nflow_mapping=Template('{\\nunquoted : \"separate\",\\nhttp://foo.com,\\nomitted value:,\\n}\\n')\nunknown_anchor=Template('value: *missing\\n')\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_static_backend_semantics.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_static_backend_semantics"),
            )
            .unwrap();

            let flow_mapping = render_yaml(py, &module.getattr("flow_mapping").unwrap())
                .expect("expected static YAML flow mapping to materialize");
            let mapping = flow_mapping.bind(py);
            assert_eq!(
                mapping
                    .get_item("unquoted")
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "separate"
            );
            assert_eq!(mapping.get_item("http://foo.com").unwrap().is_none(), true);
            assert!(mapping.get_item("omitted value").unwrap().is_none());

            let err = render_yaml(py, &module.getattr("unknown_anchor").unwrap())
                .expect_err("expected static YAML backend error");
            assert!(err.is_instance_of::<TemplateSemanticError>(py));
        });
    }

    #[test]
    fn private_json_result_payload_matches_public_json_helpers() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("rows=[{'name': 'one'}, {'name': 'two'}]\ntemplate=t'{rows}'\n"),
                pyo3::ffi::c_str!("test_bindings_json_result_payload.py"),
                pyo3::ffi::c_str!("test_bindings_json_result_payload"),
            )
            .unwrap();

            let template = module.getattr("template").unwrap();
            let (text, data) = _render_json_result_payload(py, &template).unwrap();
            let expected_text = render_json_text(py, &template).unwrap();
            let expected_data = render_json(py, &template).unwrap();

            assert_eq!(text, expected_text);
            assert!(data.bind(py).eq(expected_data.bind(py)).unwrap());
        });
    }

    #[test]
    fn private_toml_result_payload_matches_public_toml_helpers() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!("owner='platform'\ntemplate=t'owner = {owner}'\n"),
                pyo3::ffi::c_str!("test_bindings_toml_result_payload.py"),
                pyo3::ffi::c_str!("test_bindings_toml_result_payload"),
            )
            .unwrap();

            let template = module.getattr("template").unwrap();
            let (text, data) = _render_toml_result_payload(py, &template).unwrap();
            let expected_text = render_toml_text(py, &template).unwrap();
            let expected_data = render_toml(py, &template).unwrap();

            assert_eq!(text, expected_text);
            assert!(data.bind(py).eq(expected_data.bind(py)).unwrap());
        });
    }

    #[test]
    fn private_yaml_result_payload_matches_public_yaml_helpers() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "name='Alice'\nnumber=1\nvalidated=t'name: {name}\\n'\ntagged=t'value: !!str {number}'\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_result_payload.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_result_payload"),
            )
            .unwrap();

            let validated = module.getattr("validated").unwrap();
            let tagged = module.getattr("tagged").unwrap();

            let (text, data) = _render_yaml_result_payload(py, &validated).unwrap();
            let expected_text = render_yaml_text(py, &validated).unwrap();
            let expected_data = render_yaml(py, &validated).unwrap();

            assert_eq!(text, expected_text);
            assert!(data.bind(py).eq(expected_data.bind(py)).unwrap());

            let (tagged_text, tagged_data) = _render_yaml_result_payload(py, &tagged).unwrap();
            let expected_tagged_text = render_yaml_text(py, &tagged).unwrap();
            let expected_tagged_data = render_yaml(py, &tagged).unwrap();

            assert_eq!(tagged_text, expected_tagged_text);
            assert!(
                tagged_data
                    .bind(py)
                    .eq(expected_tagged_data.bind(py))
                    .unwrap()
            );
        });
    }

    #[test]
    fn private_yaml_result_payload_preserves_render_text_bytes() {
        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                pyo3::ffi::c_str!(
                    "from string.templatelib import Template\nname='Alice'\nstatic_comment=Template('# heading\\nvalue: 1\\n\\n# tail\\n')\ninterpolated=t'# heading\\nvalue: {name}\\n\\n# tail\\n'\nmulti_doc=Template('---\\n# first\\nvalue: 1\\n...\\n---\\n\\n# second\\nvalue: 2\\n')\n"
                ),
                pyo3::ffi::c_str!("test_bindings_yaml_result_payload_text.py"),
                pyo3::ffi::c_str!("test_bindings_yaml_result_payload_text"),
            )
            .unwrap();

            for name in ["static_comment", "interpolated", "multi_doc"] {
                let template = module.getattr(name).unwrap();
                let (text, _) = _render_yaml_result_payload(py, &template).unwrap();
                let expected = render_yaml_text(py, &template).unwrap();
                assert_eq!(text, expected, "{name}");
            }
        });
    }
}
