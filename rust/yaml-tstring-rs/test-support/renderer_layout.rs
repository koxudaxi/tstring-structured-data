use pyo3::prelude::*;
use saphyr::{LoadableYamlNode, MappingOwned, YamlOwned};
use tstring_pyo3_bindings::{
    extract_template,
    yaml::{render_document, render_document_data},
};
use tstring_yaml::{YamlProfile, parse_template};

fn parse_rendered_yaml(text: &str) -> Vec<YamlOwned> {
    YamlOwned::load_from_str(text).expect("expected rendered YAML to parse")
}

fn yaml_scalar_text(value: &YamlOwned) -> Option<&str> {
    match value {
        YamlOwned::Value(value) => value.as_str(),
        YamlOwned::Representation(value, _, _) => Some(value.as_str()),
        YamlOwned::Tagged(_, value) => yaml_scalar_text(value),
        _ => None,
    }
}

fn yaml_mapping(value: &YamlOwned) -> Option<&MappingOwned> {
    match value {
        YamlOwned::Mapping(mapping) => Some(mapping),
        YamlOwned::Tagged(_, value) => yaml_mapping(value),
        _ => None,
    }
}

fn yaml_mapping_entry<'a>(document: &'a YamlOwned, key: &str) -> Option<&'a YamlOwned> {
    yaml_mapping(document).and_then(|mapping| {
        mapping.iter().find_map(|(entry_key, entry_value)| {
            (yaml_scalar_text(entry_key) == Some(key)).then_some(entry_value)
        })
    })
}

#[test]
fn renders_interpolated_python_collections_with_block_first_layout() {
    Python::with_gil(|py| {
        let module = PyModule::from_code(
            py,
            pyo3::ffi::c_str!(
                "mapping={'yes': 'on', '0123': 'a # b', 'nested': {'empty_list': [], 'empty_map': {}, 'a: b': 'yes'}}\nitems=[1, 2]\nempty_mapping={}\nempty_list=[]\ntag='custom'\nanchor='root'\nblock_mapping=t'value: {mapping}\\n'\nroot_mapping=t'{mapping}\\n'\nroot_sequence=t'{items}\\n'\ndecorated_tag=t'value: !{tag} {mapping}\\n'\ndecorated_anchor=t'value: &{anchor} {items}\\n'\ndecorated_both=t'value: !{tag} &{anchor} {mapping}\\n'\nflow_sequence=t'flow: [{mapping}]\\n'\nflow_mapping=t'flow: {{k: {items}}}\\n'\nempties=t'value_map: {empty_mapping}\\nvalue_list: {empty_list}\\n'\n"
            ),
            pyo3::ffi::c_str!("test_yaml_collection_layouts.py"),
            pyo3::ffi::c_str!("test_yaml_collection_layouts"),
        )
        .unwrap();

        for (name, expected_text) in [
            (
                "block_mapping",
                "value:\n  \"yes\": \"on\"\n  \"0123\": \"a # b\"\n  \"nested\":\n    \"empty_list\": []\n    \"empty_map\": {}\n    \"a: b\": \"yes\"",
            ),
            (
                "root_mapping",
                "\"yes\": \"on\"\n\"0123\": \"a # b\"\n\"nested\":\n  \"empty_list\": []\n  \"empty_map\": {}\n  \"a: b\": \"yes\"",
            ),
            ("root_sequence", "- 1\n- 2"),
            (
                "decorated_tag",
                "value: !custom\n  \"yes\": \"on\"\n  \"0123\": \"a # b\"\n  \"nested\":\n    \"empty_list\": []\n    \"empty_map\": {}\n    \"a: b\": \"yes\"",
            ),
            ("decorated_anchor", "value: &root\n  - 1\n  - 2"),
            (
                "decorated_both",
                "value: !custom &root\n  \"yes\": \"on\"\n  \"0123\": \"a # b\"\n  \"nested\":\n    \"empty_list\": []\n    \"empty_map\": {}\n    \"a: b\": \"yes\"",
            ),
            (
                "flow_sequence",
                "flow: [ { \"yes\": \"on\", \"0123\": \"a # b\", \"nested\": { \"empty_list\": [], \"empty_map\": {}, \"a: b\": \"yes\" } } ]",
            ),
            ("flow_mapping", "flow: { k: [ 1, 2 ] }"),
            ("empties", "value_map: {}\nvalue_list: []"),
        ] {
            let template = module.getattr(name).unwrap();
            let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
            let stream = parse_template(&template).unwrap();
            let rendered = render_document(py, &template, YamlProfile::V1_2_2, &stream).unwrap();
            assert_eq!(rendered.text, expected_text, "{name}");
        }

        let decorated_both = module.getattr("decorated_both").unwrap();
        let decorated_both = extract_template(py, &decorated_both, "yaml_t/yaml_t_str").unwrap();
        let stream = parse_template(&decorated_both).unwrap();
        let rendered = render_document(py, &decorated_both, YamlProfile::V1_2_2, &stream).unwrap();
        let documents = parse_rendered_yaml(&rendered.text);
        let value = yaml_mapping_entry(&documents[0], "value").expect("value key");
        let nested = yaml_mapping_entry(value, "nested").expect("nested key");
        assert_eq!(
            yaml_scalar_text(yaml_mapping_entry(nested, "a: b").expect("a: b key")),
            Some("yes")
        );
    });
}

#[test]
fn rejects_block_formatted_payloads_in_text_only_positions() {
    Python::with_gil(|py| {
        let module = PyModule::from_code(
            py,
            pyo3::ffi::c_str!(
                "class BlockMap:\n    def __str__(self):\n        return 'nested:\\n  - 1\\n  - 2'\n\nblock = BlockMap()\nvalue_template = t'value: {block!s}\\n'\nflow_template = t'flow: [{block!s}]\\n'\n"
            ),
            pyo3::ffi::c_str!("test_yaml_formatted_block_payload.py"),
            pyo3::ffi::c_str!("test_yaml_formatted_block_payload"),
        )
        .unwrap();

        for name in ["value_template", "flow_template"] {
            let template = module.getattr(name).unwrap();
            let template = extract_template(py, &template, "yaml_t/yaml_t_str").unwrap();
            let stream = parse_template(&template).unwrap();
            let err = match render_document(py, &template, YamlProfile::V1_2_2, &stream) {
                Ok(_) => panic!("expected formatted block payload rejection"),
                Err(err) => err,
            };
            assert!(
                err.message.contains("flow-safe formatted text"),
                "{name}: {}",
                err.message
            );
        }
    });
}

#[test]
fn render_data_keeps_formatted_block_payload_semantics() {
    Python::with_gil(|py| {
        let module = PyModule::from_code(
            py,
            pyo3::ffi::c_str!(
                "class BlockMap:\n    def __str__(self):\n        return 'nested:\\n  - 1\\n  - 2'\n\nblock = BlockMap()\nroot_template = t'{block!s}\\n'\nvalue_template = t'value: {block!s}\\n'\n"
            ),
            pyo3::ffi::c_str!("test_yaml_formatted_block_payload_data.py"),
            pyo3::ffi::c_str!("test_yaml_formatted_block_payload_data"),
        )
        .unwrap();

        let root_template = module.getattr("root_template").unwrap();
        let root_template = extract_template(py, &root_template, "yaml_t/yaml_t_str").unwrap();
        let root_stream = parse_template(&root_template).unwrap();
        let root_documents =
            render_document_data(py, &root_template, YamlProfile::V1_2_2, &root_stream).unwrap();
        assert_eq!(
            yaml_mapping_entry(&root_documents[0], "nested")
                .map(|value| value.as_vec().unwrap().len()),
            Some(2)
        );

        let value_template = module.getattr("value_template").unwrap();
        let value_template = extract_template(py, &value_template, "yaml_t/yaml_t_str").unwrap();
        let value_stream = parse_template(&value_template).unwrap();
        let value_documents =
            render_document_data(py, &value_template, YamlProfile::V1_2_2, &value_stream).unwrap();
        let value = yaml_mapping_entry(&value_documents[0], "value").expect("value key");
        assert_eq!(
            yaml_mapping_entry(value, "nested").map(|value| value.as_vec().unwrap().len()),
            Some(2)
        );
    });
}
