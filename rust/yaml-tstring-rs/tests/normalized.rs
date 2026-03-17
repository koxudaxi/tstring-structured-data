use saphyr::LoadableYamlNode;
use tstring_syntax::{NormalizedDocument, NormalizedKey, NormalizedValue};
use tstring_yaml::normalize_documents;

fn parse_documents(text: &str) -> Vec<saphyr::YamlOwned> {
    saphyr::YamlOwned::load_from_str(text).expect("expected YAML test fixture to parse")
}

#[test]
fn materialized_yaml_merges_override_duplicate_keys_in_normalized_output() {
    let documents =
        parse_documents("base: &base\n  a: 1\n  b: 2\nderived:\n  <<: *base\n  b: 3\n  c: 4\n");
    let normalized = normalize_documents(&documents).unwrap();

    let NormalizedDocument::Value(NormalizedValue::Mapping(root_entries)) =
        &normalized.documents[0]
    else {
        panic!("expected normalized root mapping");
    };
    let derived = root_entries
        .iter()
        .find(|entry| entry.key == NormalizedKey::String("derived".to_owned()))
        .expect("expected derived entry");
    let NormalizedValue::Mapping(derived_entries) = &derived.value else {
        panic!("expected normalized derived mapping");
    };

    assert_eq!(derived_entries.len(), 3);
    assert_eq!(
        derived_entries[0].key,
        NormalizedKey::String("a".to_owned())
    );
    assert_eq!(
        derived_entries[1].key,
        NormalizedKey::String("b".to_owned())
    );
    assert_eq!(
        derived_entries[2].key,
        NormalizedKey::String("c".to_owned())
    );
    assert_eq!(
        derived_entries
            .iter()
            .filter(|entry| entry.key == NormalizedKey::String("b".to_owned()))
            .count(),
        1
    );
    assert_eq!(
        derived_entries[1].value,
        NormalizedValue::Integer(3_i64.into())
    );
}
