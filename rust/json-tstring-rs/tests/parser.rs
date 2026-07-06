use tstring_json::{
    JsonKeyValue, JsonProfile, JsonStaticScalarKind, JsonStringPart, JsonValueNode, check_template,
    format_template, interpolation_type_requirements, parse_template, parse_validated_template,
    requirement_for, static_structure_outline, validate_template,
};
use tstring_syntax::{
    DslType, InterpolationTypeRequirement, StructuralRole, StructuralSite, TemplateInput,
    TemplateInterpolation, TemplateSegment, TypeRequirement,
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
fn reports_contextual_interpolation_type_requirements() {
    let template = TemplateInput::from_segments(vec![
        TemplateSegment::StaticText("{".to_owned()),
        interpolation(0, "key"),
        TemplateSegment::StaticText(": ".to_owned()),
        interpolation(1, "value"),
        TemplateSegment::StaticText(", \"label\": \"".to_owned()),
        interpolation(2, "label"),
        TemplateSegment::StaticText("\"}".to_owned()),
    ]);

    assert_eq!(
        interpolation_type_requirements(&template).expect("expected type requirements"),
        vec![
            InterpolationTypeRequirement::new(0, "str", "json object key"),
            InterpolationTypeRequirement::new(
                1,
                "str | int | float | bool | None | dict[str, object] | list[object] | tuple[object, ...]",
                "json value"
            ),
            InterpolationTypeRequirement::with_site(
                2,
                "str",
                "json string fragment",
                StructuralSite::new("/label", StructuralRole::ValueFragment)
            ),
        ]
    );
}

#[test]
fn reports_structural_sites_for_static_json_paths() {
    let template = TemplateInput::from_segments(vec![
        TemplateSegment::StaticText("{\"outer\": [{\"id\": ".to_owned()),
        interpolation(0, "id"),
        TemplateSegment::StaticText(", \"name\": \"Ada ".to_owned()),
        interpolation(1, "suffix"),
        TemplateSegment::StaticText("\"}], \"a/b~c\": ".to_owned()),
        interpolation(2, "escaped"),
        TemplateSegment::StaticText("}".to_owned()),
    ]);

    assert_eq!(
        interpolation_type_requirements(&template).expect("expected type requirements"),
        vec![
            InterpolationTypeRequirement::with_site(
                0,
                "str | int | float | bool | None | dict[str, object] | list[object] | tuple[object, ...]",
                "json value",
                StructuralSite::new("/outer/0/id", StructuralRole::Value)
            ),
            InterpolationTypeRequirement::with_site(
                1,
                "str",
                "json string fragment",
                StructuralSite::new("/outer/0/name", StructuralRole::ValueFragment)
            ),
            InterpolationTypeRequirement::with_site(
                2,
                "str | int | float | bool | None | dict[str, object] | list[object] | tuple[object, ...]",
                "json value",
                StructuralSite::new("/a~1b~0c", StructuralRole::Value)
            ),
        ]
    );
}

#[test]
fn omits_structural_site_under_interpolated_json_keys() {
    let template = TemplateInput::from_segments(vec![
        TemplateSegment::StaticText("{".to_owned()),
        interpolation(0, "key"),
        TemplateSegment::StaticText(": {\"id\": ".to_owned()),
        interpolation(1, "id"),
        TemplateSegment::StaticText("}, \"prefix-".to_owned()),
        interpolation(2, "suffix"),
        TemplateSegment::StaticText("\": true}".to_owned()),
    ]);

    assert_eq!(
        interpolation_type_requirements(&template).expect("expected type requirements"),
        vec![
            InterpolationTypeRequirement::new(0, "str", "json object key"),
            InterpolationTypeRequirement::new(
                1,
                "str | int | float | bool | None | dict[str, object] | list[object] | tuple[object, ...]",
                "json value"
            ),
            InterpolationTypeRequirement::new(2, "str", "json object key fragment"),
        ]
    );
}

#[test]
fn maps_json_dsl_types_to_python_requirements() {
    let cases = [
        ("integer", "int"),
        ("number", "int | float"),
        ("string", "str"),
        ("boolean", "bool"),
        ("null", "None"),
        ("array", "list[object] | tuple[object, ...]"),
        ("object", "dict[str, object]"),
        (
            "any",
            "str | int | float | bool | None | dict[str, object] | list[object] | tuple[object, ...]",
        ),
        (
            "unknown",
            "str | int | float | bool | None | dict[str, object] | list[object] | tuple[object, ...]",
        ),
    ];

    for (name, expected) in cases {
        assert_eq!(
            requirement_for(&DslType::new(name), JsonProfile::default()),
            TypeRequirement::new(expected, Vec::new()),
            "{name}"
        );
    }
    assert_eq!(
        requirement_for(
            &DslType::with_format("string", "date-time"),
            JsonProfile::default()
        ),
        TypeRequirement::new("str", Vec::new())
    );
}

#[test]
fn reports_static_json_structure_outline_with_spans() {
    let template = TemplateInput::from_segments(vec![
        TemplateSegment::StaticText(
            "{\"name\": \"Ada\", \"age\": 42, \"nested\": {\"active\": true}, \"dyn-".to_owned(),
        ),
        interpolation(0, "suffix"),
        TemplateSegment::StaticText("\": false}".to_owned()),
    ]);

    let outline = static_structure_outline(&template).expect("expected outline");
    assert_eq!(outline.objects.len(), 2);

    let root = &outline.objects[0];
    assert_eq!(root.pointer.as_deref(), Some(""));
    assert!(root.has_interpolated_key);
    assert_eq!(
        root.static_keys
            .iter()
            .map(|key| key.name.as_str())
            .collect::<Vec<_>>(),
        vec!["name", "age", "nested"]
    );
    assert_eq!(root.static_keys[0].span.start.token_index, 0);
    assert_eq!(root.static_keys[0].span.start.offset, 1);
    assert_eq!(root.static_keys[0].span.end.offset, 7);
    assert_eq!(
        root.static_scalar_values
            .iter()
            .filter_map(|value| value.key.as_deref().zip(Some(value.kind)))
            .collect::<Vec<_>>(),
        vec![
            ("name", JsonStaticScalarKind::String),
            ("age", JsonStaticScalarKind::Integer),
        ]
    );

    let nested = &outline.objects[1];
    assert_eq!(nested.pointer.as_deref(), Some("/nested"));
    assert!(!nested.has_interpolated_key);
    assert_eq!(nested.static_keys[0].name, "active");
    assert_eq!(
        nested.static_scalar_values[0].kind,
        JsonStaticScalarKind::Boolean
    );
    assert_eq!(
        nested.static_scalar_values[0].pointer.as_deref(),
        Some("/nested/active")
    );
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
