use tstring_syntax::{TemplateInput, TemplateInterpolation, TemplateSegment};
use tstring_toml::{
    check_template, format_template, parse_template, TomlStatementNode, TomlValueNode,
};

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

#[test]
fn checks_valid_toml_templates() {
    let template = TemplateInput::from_segments(vec![
        TemplateSegment::StaticText("title = ".to_owned()),
        TemplateSegment::Interpolation(TemplateInterpolation {
            expression: "title".to_owned(),
            conversion: None,
            format_spec: String::new(),
            interpolation_index: 0,
            raw_source: Some("{title}".to_owned()),
        }),
        TemplateSegment::StaticText("\nname = \"Hello ".to_owned()),
        TemplateSegment::Interpolation(TemplateInterpolation {
            expression: "name".to_owned(),
            conversion: Some("s".to_owned()),
            format_spec: String::new(),
            interpolation_index: 1,
            raw_source: Some("{name!s}".to_owned()),
        }),
        TemplateSegment::StaticText("\"\n".to_owned()),
    ]);

    check_template(&template).expect("expected check success");
}

#[test]
fn formats_toml_templates_with_raw_interpolations() {
    let template = TemplateInput::from_segments(vec![
        TemplateSegment::StaticText("[".to_owned()),
        TemplateSegment::Interpolation(TemplateInterpolation {
            expression: "table".to_owned(),
            conversion: None,
            format_spec: String::new(),
            interpolation_index: 0,
            raw_source: Some("{table}".to_owned()),
        }),
        TemplateSegment::StaticText("]\nmessage = \"Hello ".to_owned()),
        TemplateSegment::Interpolation(TemplateInterpolation {
            expression: "name".to_owned(),
            conversion: Some("r".to_owned()),
            format_spec: ">5".to_owned(),
            interpolation_index: 1,
            raw_source: Some("{name!r:>5}".to_owned()),
        }),
        TemplateSegment::StaticText("\"\nitems = [".to_owned()),
        TemplateSegment::Interpolation(TemplateInterpolation {
            expression: "item".to_owned(),
            conversion: None,
            format_spec: String::new(),
            interpolation_index: 2,
            raw_source: Some("{item}".to_owned()),
        }),
        TemplateSegment::StaticText("]\n".to_owned()),
    ]);

    assert_eq!(
        format_template(&template).expect("expected format success"),
        "[{table}]\nmessage = \"Hello {name!r:>5}\"\nitems = [{item}]"
    );
}

#[test]
fn format_requires_raw_source_for_toml_interpolations() {
    let template = TemplateInput::from_segments(vec![
        TemplateSegment::StaticText("value = ".to_owned()),
        interpolation(0, "value"),
        TemplateSegment::StaticText("\n".to_owned()),
    ]);

    let error = format_template(&template).expect_err("expected format failure");
    assert_eq!(error.kind, tstring_syntax::ErrorKind::Semantic);
    assert_eq!(error.diagnostics[0].code, "toml.format");
}
