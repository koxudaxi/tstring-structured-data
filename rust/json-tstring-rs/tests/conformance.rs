use std::fs;
use std::path::{Path, PathBuf};

use toml::Value;
use tstring_json::{JsonProfile, parse_template_with_profile};
use tstring_syntax::{TemplateInput, TemplateSegment};

struct Case {
    case_id: String,
    expected: String,
    input_path: String,
    execution_layer: String,
}

#[test]
fn json_conformance_manifest_cases_match_parser_expectations() {
    for case in load_cases("rfc8259") {
        if case.execution_layer != "rust" && case.execution_layer != "both" {
            continue;
        }

        let input = fs::read_to_string(conformance_root().join(&case.input_path))
            .unwrap_or_else(|_| panic!("missing input for {}", case.case_id));
        let template = TemplateInput::from_segments(vec![TemplateSegment::StaticText(input)]);
        let parsed = parse_template_with_profile(&template, JsonProfile::Rfc8259);

        match case.expected.as_str() {
            "accept" => assert!(parsed.is_ok(), "expected accept for {}", case.case_id),
            "reject" => assert!(parsed.is_err(), "expected reject for {}", case.case_id),
            other => panic!("unsupported expected value {other}"),
        }
    }
}

fn load_cases(profile: &str) -> Vec<Case> {
    let profiles = fs::read_to_string(conformance_root().join("profiles.toml")).unwrap();
    let profiles = toml::from_str::<Value>(&profiles).unwrap();
    let manifest_path = profiles["profiles"][profile]["manifest_path"]
        .as_str()
        .unwrap();
    let manifest = fs::read_to_string(conformance_root().join(manifest_path)).unwrap();
    let value = toml::from_str::<Value>(&manifest).unwrap();
    value["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| Case {
            case_id: entry["case_id"].as_str().unwrap().to_owned(),
            expected: entry["expected"].as_str().unwrap().to_owned(),
            input_path: entry["input_path"].as_str().unwrap().to_owned(),
            execution_layer: entry["execution_layer"].as_str().unwrap().to_owned(),
        })
        .collect()
}

fn conformance_root() -> PathBuf {
    repo_root().join("conformance/json")
}

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
}
