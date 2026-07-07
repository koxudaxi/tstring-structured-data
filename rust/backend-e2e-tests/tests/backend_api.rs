use tstring_json as backend_json;
use tstring_syntax::{
    DslType, InterpolationTypeRequirement, StructuralRole, StructuralSite, TemplateInput,
    TemplateInterpolation, TemplateSegment, TypeRequirement,
};
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
fn json_backend_structural_site_public_api_smoke_test() {
    let template = TemplateInput::from_segments(vec![
        TemplateSegment::StaticText("{\"items\": [{\"id\": ".to_owned()),
        interpolation(0, "id", "{id}"),
        TemplateSegment::StaticText("}], \"name\": \"Ada\"}".to_owned()),
    ]);

    assert_eq!(
        backend_json::interpolation_type_requirements(&template)
            .expect("expected json type requirements"),
        vec![InterpolationTypeRequirement::with_site(
            0,
            "str | int | float | bool | None | dict[str, object] | list[object] | tuple[object, ...]",
            "json value",
            StructuralSite::new("/items/0/id", StructuralRole::Value)
        )]
    );
    assert_eq!(
        backend_json::requirement_for(
            &DslType::new("integer"),
            backend_json::JsonProfile::default()
        ),
        TypeRequirement::new("int", Vec::new())
    );

    let outline = backend_json::static_structure_outline(&template).expect("expected outline");
    assert_eq!(outline.objects[0].pointer.as_deref(), Some(""));
    assert_eq!(outline.objects[1].pointer.as_deref(), Some("/items/0"));
    assert_eq!(outline.objects[0].static_keys[0].name, "items");
    assert_eq!(outline.objects[0].static_keys[1].name, "name");
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
        backend_toml::interpolation_type_requirements(&template)
            .expect("expected toml type requirements"),
        vec![
            InterpolationTypeRequirement::new(
                0,
                "str | int | float | bool | datetime.date | datetime.time | datetime.datetime | list[object] | tuple[object, ...] | dict[str, object]",
                "toml value"
            ),
            InterpolationTypeRequirement::new(1, "str", "toml string fragment"),
        ]
    );
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
        backend_yaml::interpolation_type_requirements(&template)
            .expect("expected yaml type requirements"),
        vec![
            InterpolationTypeRequirement::new(
                0,
                "str | int | float | bool | None | datetime.date | datetime.time | datetime.datetime | list[object] | tuple[object, ...] | dict[object, object]",
                "yaml value"
            ),
            InterpolationTypeRequirement::new(
                1,
                "str | int | float | bool | None | datetime.date | datetime.time | datetime.datetime | list[object] | tuple[object, ...] | dict[object, object]",
                "yaml value"
            ),
            InterpolationTypeRequirement::new(2, "str", "yaml scalar fragment"),
        ]
    );
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
    let first = error
        .diagnostics
        .first()
        .expect("expected at least one diagnostic");
    assert_eq!(first.code, "json.parse");
    assert!(first.span.is_some());
}
