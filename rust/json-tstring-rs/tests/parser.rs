use tstring_json::{
    JsonKeyValue, JsonStringPart, JsonValueNode, check_template, format_template, parse_template,
    parse_validated_template, validate_template,
};
use tstring_syntax::{TemplateInput, TemplateInterpolation, TemplateSegment};

fn interpolation(index: usize, expression: &str) -> TemplateSegment {
    TemplateSegment::Interpolation(TemplateInterpolation {
        expression: expression.to_owned(),
        conversion: None,
        format_spec: String::new(),
        interpolation_index: index,
        raw_source: None,
    })
}

#[test]
fn parses_json_with_interpolated_key_and_value_segments() {
    let template = TemplateInput::from_segments(vec![
        TemplateSegment::StaticText("{\"name-".to_owned()),
        interpolation(0, "left"),
        TemplateSegment::StaticText("\": ".to_owned()),
        interpolation(1, "right"),
        TemplateSegment::StaticText("}".to_owned()),
    ]);

    let document = parse_template(&template).expect("expected JSON parse success");
    let JsonValueNode::Object(object) = document.value else {
        panic!("expected JSON object");
    };
    assert_eq!(object.members.len(), 1);
    let JsonKeyValue::String(key) = &object.members[0].key.value else {
        panic!("expected promoted JSON string key");
    };
    assert!(matches!(key.chunks[1], JsonStringPart::Interpolation(_)));
    assert!(matches!(
        object.members[0].value,
        JsonValueNode::Interpolation(_)
    ));
}

#[test]
fn json_parse_errors_include_spans() {
    let template = TemplateInput::from_segments(vec![TemplateSegment::StaticText(
        "{\"a\": 1 trailing}".to_owned(),
    )]);

    let error = parse_template(&template).expect_err("expected JSON parse failure");
    assert_eq!(error.diagnostics[0].code, "json.parse");
    assert!(error.diagnostics[0].span.is_some());
}

#[test]
fn checks_valid_json_templates() {
    let template = TemplateInput::from_segments(vec![
        TemplateSegment::StaticText("{\"name\": ".to_owned()),
        interpolation(0, "name"),
        TemplateSegment::StaticText(", \"message\": \"hello ".to_owned()),
        TemplateSegment::Interpolation(TemplateInterpolation {
            expression: "user".to_owned(),
            conversion: Some("r".to_owned()),
            format_spec: ">5".to_owned(),
            interpolation_index: 1,
            raw_source: Some("{user!r:>5}".to_owned()),
        }),
        TemplateSegment::StaticText("\"}".to_owned()),
    ]);

    check_template(&template).expect("expected check success");
}

#[test]
fn validates_json_templates_with_supported_interpolations() {
    let template = TemplateInput::from_segments(vec![
        TemplateSegment::StaticText("{\"name\": ".to_owned()),
        interpolation(0, "name"),
        TemplateSegment::StaticText(", \"active\": true}".to_owned()),
    ]);

    validate_template(&template).expect("expected validate success");
    parse_validated_template(&template).expect("expected validated parse success");
}

#[test]
fn formats_json_templates_with_raw_interpolations() {
    let template = TemplateInput::from_segments(vec![
        TemplateSegment::StaticText("{".to_owned()),
        TemplateSegment::Interpolation(TemplateInterpolation {
            expression: "key".to_owned(),
            conversion: None,
            format_spec: String::new(),
            interpolation_index: 0,
            raw_source: Some("{key}".to_owned()),
        }),
        TemplateSegment::StaticText(": ".to_owned()),
        TemplateSegment::Interpolation(TemplateInterpolation {
            expression: "value".to_owned(),
            conversion: None,
            format_spec: String::new(),
            interpolation_index: 1,
            raw_source: Some("{value}".to_owned()),
        }),
        TemplateSegment::StaticText(", \"greeting\": \"hi ".to_owned()),
        TemplateSegment::Interpolation(TemplateInterpolation {
            expression: "user".to_owned(),
            conversion: Some("r".to_owned()),
            format_spec: ">5".to_owned(),
            interpolation_index: 2,
            raw_source: Some("{user!r:>5}".to_owned()),
        }),
        TemplateSegment::StaticText("\"}".to_owned()),
    ]);

    assert_eq!(
        format_template(&template).expect("expected format success"),
        r#"{{key}: {value}, "greeting": "hi {user!r:>5}"}"#
    );
}

#[test]
fn format_requires_raw_source_for_interpolations() {
    let template = TemplateInput::from_segments(vec![
        TemplateSegment::StaticText("{\"name\": ".to_owned()),
        interpolation(0, "name"),
        TemplateSegment::StaticText("}".to_owned()),
    ]);

    let error = format_template(&template).expect_err("expected format failure");
    assert_eq!(error.kind, tstring_syntax::ErrorKind::Semantic);
    assert_eq!(error.diagnostics[0].code, "json.format");
}
