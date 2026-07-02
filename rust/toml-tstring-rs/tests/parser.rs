use tstring_syntax::{
    InterpolationTypeRequirement, TemplateInput, TemplateInterpolation, TemplateSegment,
};
use tstring_toml::{
    TomlStatementNode, TomlValueNode, check_template, format_template,
    interpolation_type_requirements, parse_template, parse_validated_template, validate_template,
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
fn validates_toml_templates_with_supported_interpolations() {
    let template = TemplateInput::from_segments(vec![
        TemplateSegment::StaticText("title = ".to_owned()),
        TemplateSegment::Interpolation(TemplateInterpolation {
            expression: "title".to_owned(),
            conversion: None,
            format_spec: String::new(),
            interpolation_index: 0,
            raw_source: Some("{title}".to_owned()),
        }),
        TemplateSegment::StaticText("\n".to_owned()),
    ]);

    validate_template(&template).expect("expected validate success");
    parse_validated_template(&template).expect("expected validated parse success");
}

#[test]
fn reports_contextual_interpolation_type_requirements() {
    let template = TemplateInput::from_segments(vec![
        interpolation(0, "table"),
        TemplateSegment::StaticText(".title = ".to_owned()),
        interpolation(1, "value"),
        TemplateSegment::StaticText("\nmessage = \"Hello ".to_owned()),
        interpolation(2, "fragment"),
        TemplateSegment::StaticText("\"\nmeta = { ".to_owned()),
        interpolation(3, "inline_key"),
        TemplateSegment::StaticText(" = ".to_owned()),
        interpolation(4, "inline_value"),
        TemplateSegment::StaticText(" }\n".to_owned()),
    ]);

    assert_eq!(
        interpolation_type_requirements(&template).expect("expected type requirements"),
        vec![
            InterpolationTypeRequirement::new(0, "str", "toml key"),
            InterpolationTypeRequirement::new(
                1,
                "str | int | float | bool | datetime.date | datetime.time | datetime.datetime | list[object] | dict[str, object]",
                "toml value"
            ),
            InterpolationTypeRequirement::new(2, "str", "toml string fragment"),
            InterpolationTypeRequirement::new(3, "str", "toml key"),
            InterpolationTypeRequirement::new(
                4,
                "str | int | float | bool | datetime.date | datetime.time | datetime.datetime | list[object] | dict[str, object]",
                "toml value"
            ),
        ]
    );
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
