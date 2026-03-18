use tstring_syntax::{TemplateInput, TemplateInterpolation, TemplateSegment};
use tstring_yaml::{
    YamlValueNode, check_template, format_template, parse_template, validate_template,
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

#[test]
fn checks_valid_yaml_templates() {
    let template = TemplateInput::from_segments(vec![
        TemplateSegment::StaticText("name: ".to_owned()),
        TemplateSegment::Interpolation(TemplateInterpolation {
            expression: "name".to_owned(),
            conversion: None,
            format_spec: String::new(),
            interpolation_index: 0,
            raw_source: Some("{name}".to_owned()),
        }),
        TemplateSegment::StaticText("\nmeta:\n  team: \"".to_owned()),
        TemplateSegment::Interpolation(TemplateInterpolation {
            expression: "team".to_owned(),
            conversion: Some("s".to_owned()),
            format_spec: String::new(),
            interpolation_index: 1,
            raw_source: Some("{team!s}".to_owned()),
        }),
        TemplateSegment::StaticText("\"\n".to_owned()),
    ]);

    check_template(&template).expect("expected check success");
}

#[test]
fn rejects_plain_scalars_that_mix_whitespace_and_interpolation() {
    let template = TemplateInput::from_segments(vec![
        TemplateSegment::StaticText("replicas: fdsa fff fds".to_owned()),
        TemplateSegment::Interpolation(TemplateInterpolation {
            expression: "replicas".to_owned(),
            conversion: None,
            format_spec: String::new(),
            interpolation_index: 0,
            raw_source: Some("{replicas}".to_owned()),
        }),
        TemplateSegment::StaticText("\n".to_owned()),
    ]);

    let error = check_template(&template).expect_err("expected YAML validation failure");
    assert_eq!(error.diagnostics[0].code, "yaml.parse");
    assert!(
        error
            .message
            .contains("Quote YAML plain scalars that mix whitespace and interpolations")
    );
}

#[test]
fn validates_token_like_plain_scalars_with_interpolation() {
    let template = TemplateInput::from_segments(vec![
        TemplateSegment::StaticText("plain: item-".to_owned()),
        TemplateSegment::Interpolation(TemplateInterpolation {
            expression: "user".to_owned(),
            conversion: None,
            format_spec: String::new(),
            interpolation_index: 0,
            raw_source: Some("{user}".to_owned()),
        }),
        TemplateSegment::StaticText("\n".to_owned()),
    ]);

    validate_template(&template).expect("expected YAML validation success");
}

#[test]
fn formats_yaml_templates_with_raw_interpolations() {
    let template = TemplateInput::from_segments(vec![
        TemplateSegment::StaticText("name: ".to_owned()),
        TemplateSegment::Interpolation(TemplateInterpolation {
            expression: "name".to_owned(),
            conversion: None,
            format_spec: String::new(),
            interpolation_index: 0,
            raw_source: Some("{name}".to_owned()),
        }),
        TemplateSegment::StaticText("\nmessage: \"Hello ".to_owned()),
        TemplateSegment::Interpolation(TemplateInterpolation {
            expression: "user".to_owned(),
            conversion: Some("r".to_owned()),
            format_spec: ">5".to_owned(),
            interpolation_index: 1,
            raw_source: Some("{user!r:>5}".to_owned()),
        }),
        TemplateSegment::StaticText("\"\nitems:\n  - ".to_owned()),
        TemplateSegment::Interpolation(TemplateInterpolation {
            expression: "item".to_owned(),
            conversion: None,
            format_spec: String::new(),
            interpolation_index: 2,
            raw_source: Some("{item}".to_owned()),
        }),
        TemplateSegment::StaticText("\n".to_owned()),
    ]);

    assert_eq!(
        format_template(&template).expect("expected format success"),
        "name: {name}\nmessage: \"Hello {user!r:>5}\"\nitems:\n  - {item}"
    );
}

#[test]
fn format_requires_raw_source_for_yaml_interpolations() {
    let template = TemplateInput::from_segments(vec![
        TemplateSegment::StaticText("name: ".to_owned()),
        interpolation(0, "name"),
        TemplateSegment::StaticText("\n".to_owned()),
    ]);

    let error = format_template(&template).expect_err("expected format failure");
    assert_eq!(error.kind, tstring_syntax::ErrorKind::Semantic);
    assert_eq!(error.diagnostics[0].code, "yaml.format");
}

#[test]
fn formats_explicit_complex_keys_as_flow_when_required() {
    let template = TemplateInput::from_segments(vec![TemplateSegment::StaticText(
        "?\n  a:\n    - 1\n    - 2\n: 1\n".to_owned(),
    )]);

    assert_eq!(
        format_template(&template).expect("expected format success"),
        "? { a: [ 1, 2 ] }\n: 1"
    );
}

#[test]
fn preserves_required_newline_for_empty_block_scalars() {
    let template =
        TemplateInput::from_segments(vec![TemplateSegment::StaticText("value: |\n".to_owned())]);

    assert_eq!(
        format_template(&template).expect("expected format success"),
        "value: |\n"
    );
}
