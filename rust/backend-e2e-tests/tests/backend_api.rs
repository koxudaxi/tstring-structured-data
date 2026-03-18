use tstring_json as backend_json;
use tstring_syntax::{TemplateInput, TemplateInterpolation, TemplateSegment};
use tstring_toml as backend_toml;
use tstring_yaml as backend_yaml;

fn interpolation(index: usize, expression: &str, raw_source: &str) -> TemplateSegment {
    TemplateSegment::Interpolation(TemplateInterpolation {
        expression: expression.to_owned(),
        conversion: None,
        format_spec: String::new(),
        interpolation_index: index,
        raw_source: Some(raw_source.to_owned()),
    })
}

#[test]
fn json_backend_public_api_smoke_test() {
    let template = TemplateInput::from_segments(vec![
        TemplateSegment::StaticText("{\"name\": ".to_owned()),
        interpolation(0, "name", "{name}"),
        TemplateSegment::StaticText(", \"message\": \"Hello ".to_owned()),
        TemplateSegment::Interpolation(TemplateInterpolation {
            expression: "user".to_owned(),
            conversion: Some("r".to_owned()),
            format_spec: ">5".to_owned(),
            interpolation_index: 1,
            raw_source: Some("{user!r:>5}".to_owned()),
        }),
        TemplateSegment::StaticText("\"}".to_owned()),
    ]);

    backend_json::check_template(&template).expect("expected json check success");
    assert_eq!(
        backend_json::format_template(&template).expect("expected json format success"),
        r#"{"name": {name}, "message": "Hello {user!r:>5}"}"#
    );
}

#[test]
fn toml_backend_public_api_smoke_test() {
    let template = TemplateInput::from_segments(vec![
        TemplateSegment::StaticText("title = ".to_owned()),
        interpolation(0, "title", "{title}"),
        TemplateSegment::StaticText("\nmessage = \"Hello ".to_owned()),
        TemplateSegment::Interpolation(TemplateInterpolation {
            expression: "user".to_owned(),
            conversion: Some("s".to_owned()),
            format_spec: String::new(),
            interpolation_index: 1,
            raw_source: Some("{user!s}".to_owned()),
        }),
        TemplateSegment::StaticText("\"\n".to_owned()),
    ]);

    backend_toml::check_template(&template).expect("expected toml check success");
    assert_eq!(
        backend_toml::format_template(&template).expect("expected toml format success"),
        "title = {title}\nmessage = \"Hello {user!s}\""
    );
}

#[test]
fn yaml_backend_public_api_smoke_test() {
    let template = TemplateInput::from_segments(vec![
        TemplateSegment::StaticText("name: ".to_owned()),
        interpolation(0, "name", "{name}"),
        TemplateSegment::StaticText("\nitems:\n  - ".to_owned()),
        interpolation(1, "item", "{item}"),
        TemplateSegment::StaticText("\nmessage: \"Hello ".to_owned()),
        TemplateSegment::Interpolation(TemplateInterpolation {
            expression: "user".to_owned(),
            conversion: Some("r".to_owned()),
            format_spec: ">5".to_owned(),
            interpolation_index: 2,
            raw_source: Some("{user!r:>5}".to_owned()),
        }),
        TemplateSegment::StaticText("\"\n".to_owned()),
    ]);

    backend_yaml::check_template(&template).expect("expected yaml check success");
    assert_eq!(
        backend_yaml::format_template(&template).expect("expected yaml format success"),
        "name: {name}\nitems:\n  - {item}\nmessage: \"Hello {user!r:>5}\""
    );
}

#[test]
fn check_reports_spans_for_invalid_templates_end_to_end() {
    let template =
        TemplateInput::from_segments(vec![TemplateSegment::StaticText("{\"name\": ]".to_owned())]);

    let error = backend_json::check_template(&template).expect_err("expected json parse failure");
    assert_eq!(error.diagnostics[0].code, "json.parse");
    assert!(error.diagnostics[0].span.is_some());
}
