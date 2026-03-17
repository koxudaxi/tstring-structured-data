use tstring_syntax::{TemplateInput, TemplateInterpolation, TemplateSegment};
use tstring_toml::{TomlStatementNode, TomlValueNode, parse_template};

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
fn parses_toml_assignments_and_interpolations() {
    let template = TemplateInput::from_segments(vec![
        TemplateSegment::StaticText("title = \"Hello ".to_owned()),
        interpolation(0, "name"),
        TemplateSegment::StaticText("\"\n".to_owned()),
    ]);

    let document = parse_template(&template).expect("expected TOML parse success");
    let TomlStatementNode::Assignment(assignment) = &document.statements[0] else {
        panic!("expected TOML assignment");
    };
    assert!(matches!(assignment.value, TomlValueNode::String(_)));
}

#[test]
fn toml_parse_errors_include_spans() {
    let template = TemplateInput::from_segments(vec![TemplateSegment::StaticText(
        "value = \"a\nb\"".to_owned(),
    )]);

    let error = parse_template(&template).expect_err("expected TOML parse failure");
    assert_eq!(error.diagnostics[0].code, "toml.parse");
    assert!(error.diagnostics[0].span.is_some());
}
