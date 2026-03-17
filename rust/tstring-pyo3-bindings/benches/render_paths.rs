use criterion::{Criterion, criterion_group, criterion_main};
use pyo3::prelude::*;
use pyo3::types::PyModule;
use std::sync::Arc;
use tstring_pyo3_bindings::{
    extract_template, json as json_backend, toml as toml_backend, yaml as yaml_backend,
};

fn benchmark_json_metadata_render(criterion: &mut Criterion) {
    let (template, node) = Python::with_gil(|py| {
        let module = PyModule::from_code(
            py,
            pyo3::ffi::c_str!(
                "value=3.14159\nlabel='service'\nitems=[1, 2, 3]\nmeta={'region': 'us-east-1', 'count': 3}\ntemplate=t'{\"format\": {value:.2f}, \"label\": \"{label!s}\", \"items\": {items}, \"meta\": {meta}, \"fragment\": \"pi={value:.2f}\"}'\n"
            ),
            pyo3::ffi::c_str!("bench_json_metadata.py"),
            pyo3::ffi::c_str!("bench_json_metadata"),
        )
        .unwrap();
        let template =
            extract_template(py, &module.getattr("template").unwrap(), "render_json").unwrap();
        let node = Arc::new(tstring_json::parse_template(template.input()).unwrap());
        (template, node)
    });

    criterion.bench_function("json_metadata_render", |bench| {
        bench.iter(|| {
            Python::with_gil(|py| {
                json_backend::render_document(py, &template, node.as_ref()).unwrap();
            });
        });
    });
}

fn benchmark_toml_metadata_render(criterion: &mut Criterion) {
    let (template, node) = Python::with_gil(|py| {
        let module = PyModule::from_code(
            py,
            pyo3::ffi::c_str!(
                "from datetime import UTC, datetime\nratio=3.14159\nlabel='service'\nmeta={'region': 'us-east-1', 'count': 3}\ncreated=datetime(2024, 1, 2, 3, 4, 5, tzinfo=UTC)\ntemplate=t'format = {ratio:.2f}\\nlabel = \"{label!s}\"\\nmeta = {meta}\\ncreated = {created}\\nfragment = \"pi={ratio:.2f}\"\\n'\n"
            ),
            pyo3::ffi::c_str!("bench_toml_metadata.py"),
            pyo3::ffi::c_str!("bench_toml_metadata"),
        )
        .unwrap();
        let template =
            extract_template(py, &module.getattr("template").unwrap(), "render_toml").unwrap();
        let node = Arc::new(tstring_toml::parse_template(template.input()).unwrap());
        (template, node)
    });

    criterion.bench_function("toml_metadata_render", |bench| {
        bench.iter(|| {
            Python::with_gil(|py| {
                toml_backend::render_document(py, &template, node.as_ref()).unwrap();
            });
        });
    });
}

fn benchmark_toml_dynamic_header_render(criterion: &mut Criterion) {
    let (template, node) = Python::with_gil(|py| {
        let module = PyModule::from_code(
            py,
            pyo3::ffi::c_str!(
                "env='prod'\nregion='us-east-1'\nservice='api'\nversion='v1'\ntemplate=t'[services.{env}.{region}.{service}.{version}]\\nname = \"edge\"\\ncount = 3\\n'\n"
            ),
            pyo3::ffi::c_str!("bench_toml_dynamic_header.py"),
            pyo3::ffi::c_str!("bench_toml_dynamic_header"),
        )
        .unwrap();
        let template =
            extract_template(py, &module.getattr("template").unwrap(), "render_toml").unwrap();
        let node = Arc::new(tstring_toml::parse_template(template.input()).unwrap());
        (template, node)
    });

    criterion.bench_function("toml_dynamic_header_render", |bench| {
        bench.iter(|| {
            Python::with_gil(|py| {
                toml_backend::render_document(py, &template, node.as_ref()).unwrap();
            });
        });
    });
}

fn benchmark_yaml_metadata_render(criterion: &mut Criterion) {
    let (template, node) = Python::with_gil(|py| {
        let module = PyModule::from_code(
            py,
            pyo3::ffi::c_str!(
                "ratio=3.14159\nlabel='service'\nitems=[1, 2, 3]\nmeta={'region': 'us-east-1', 'count': 3}\ntemplate=t'format: {ratio:.2f}\\nlabel: \"{label!s}\"\\nitems: {items}\\nmeta: {meta}\\nfragment: \"pi={ratio:.2f}\"\\n'\n"
            ),
            pyo3::ffi::c_str!("bench_yaml_metadata.py"),
            pyo3::ffi::c_str!("bench_yaml_metadata"),
        )
        .unwrap();
        let template =
            extract_template(py, &module.getattr("template").unwrap(), "render_yaml").unwrap();
        let node = Arc::new(tstring_yaml::parse_template(template.input()).unwrap());
        (template, node)
    });

    criterion.bench_function("yaml_metadata_render", |bench| {
        bench.iter(|| {
            Python::with_gil(|py| {
                yaml_backend::render_document(py, &template, node.as_ref()).unwrap();
            });
        });
    });
}

criterion_group!(
    benches,
    benchmark_json_metadata_render,
    benchmark_toml_metadata_render,
    benchmark_toml_dynamic_header_render,
    benchmark_yaml_metadata_render
);
criterion_main!(benches);
