use tstring_syntax::{TemplateInput, TemplateInterpolation, TemplateSegment};
use tstring_yaml::{YamlValueNode, parse_template};

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
fn parses_yaml_mapping_with_interpolated_scalar_value() {
    let template = TemplateInput::from_segments(vec![
        TemplateSegment::StaticText("name: ".to_owned()),
        interpolation(0, "name"),
        TemplateSegment::StaticText("\n".to_owned()),
    ]);

    let stream = parse_template(&template).expect("expected YAML parse success");
    assert_eq!(stream.documents.len(), 1);
    assert!(matches!(
        stream.documents[0].value,
        YamlValueNode::Mapping(_)
    ));
}

#[test]
fn yaml_parse_errors_include_spans() {
    let template =
        TemplateInput::from_segments(vec![TemplateSegment::StaticText("a:\n\t- 1\n".to_owned())]);

    let error = parse_template(&template).expect_err("expected YAML parse failure");
    assert_eq!(error.diagnostics[0].code, "yaml.parse");
    assert!(error.diagnostics[0].span.is_some());
}
