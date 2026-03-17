use tstring_json::{JsonKeyValue, JsonStringPart, JsonValueNode, parse_template};
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
