from __future__ import annotations

from string.templatelib import Template
from typing import TypeIs

import pytest
from inline_snapshot import snapshot
from tstring_core import (
    Diagnostic,
    DiagnosticSeverity,
    FragmentGroup,
    JsonValue,
    SlotContext,
    SourcePosition,
    SourceSpan,
    StaticTextToken,
    TemplateError,
    TemplateParseError,
    TemplateSemanticError,
    UnrepresentableValueError,
    flatten_template,
    tokenize_template,
)
from tstring_core import (
    RenderResult as CoreRenderResult,
)

import json_tstring
from json_tstring import RenderResult, _slots, render_data, render_result, render_text

type JsonObject = dict[str, JsonValue]


def _is_json_object(value: JsonValue) -> TypeIs[JsonObject]:
    return isinstance(value, dict)


def _expect_json_object(value: JsonValue) -> JsonObject:
    assert _is_json_object(value)
    return value


def _expect_json_string(value: JsonValue) -> str:
    assert isinstance(value, str)
    return value


class BadStringValue:
    def __str__(self) -> str:
        raise ValueError("cannot stringify")


class NestedJsonFragment:
    def __str__(self) -> str:
        inner = "inner"
        return _expect_json_string(render_data(t'"{inner}"'))


def test_json_cache_reuses_static_structure_without_stale_expression_labels() -> None:
    bad_value_one = {1}
    bad_value_two = {2}

    with pytest.raises(UnrepresentableValueError, match="bad_value_one"):
        render_text(t'{{"value": {bad_value_one}}}')

    with pytest.raises(UnrepresentableValueError, match="bad_value_two"):
        render_text(t'{{"value": {bad_value_two}}}')


def test_json_end_to_end_parser_first_positions() -> None:
    key = "user"
    left = "prefix"
    right = "suffix"
    payload = {"enabled": True, "count": 2}

    template = t"""
    {{
      {key}: {payload},
      "prefix-{left}": "item-{right}",
      "label": {left}-{right}
    }}
    """

    assert {
        "data": render_data(template),
        "text": render_text(template),
    } == snapshot(
        {
            "data": {
                "user": {"enabled": True, "count": 2},
                "prefix-prefix": "item-suffix",
                "label": "prefix-suffix",
            },
            "text": (
                '{"user": {"enabled": true, "count": 2}, '
                '"prefix-prefix": "item-suffix", "label": "prefix-suffix"}'
            ),
        }
    )


def test_json_promoted_fragments_and_top_level_values() -> None:
    left = "prefix"
    right = "suffix"
    rows = [{"name": "one"}, {"name": "two"}]

    assert {
        "fragment": render_text(t'{{"label": {left}-{right}}}'),
        "text": render_text(t"{rows}"),
        "data": render_data(t"{rows}"),
    } == snapshot(
        {
            "fragment": '{"label": "prefix-suffix"}',
            "text": '[{"name": "one"}, {"name": "two"}]',
            "data": [{"name": "one"}, {"name": "two"}],
        }
    )


def test_json_quoted_key_fragments_and_top_level_scalars() -> None:
    left = "prefix"
    right = "suffix"

    assert {
        "data": render_data(t'{{"{left}-{right}": {1}, "value": {True}}}'),
        "scalar": render_data(t"{1}"),
        "scalar_text": render_text(t"{1}"),
    } == snapshot(
        {
            "data": {"prefix-suffix": 1, "value": True},
            "scalar": 1,
            "scalar_text": "1",
        }
    )


def test_json_unicode_surrogate_pairs_follow_rfc_8259() -> None:
    assert render_data(t'"\\uD834\\uDD1E"') == snapshot("𝄞")


def test_json_rfc_8259_image_example_round_trip() -> None:
    template = t"""{{
      "Image": {{
        "Width": 800,
        "Height": 600,
        "Title": "View from 15th Floor",
        "Thumbnail": {{
          "Url": "http://www.example.com/image/481989943",
          "Height": 125,
          "Width": 100
        }},
        "Animated": false,
        "IDs": [116, 943, 234, 38793]
      }}
    }}"""

    assert {
        "data": render_data(template),
        "text": render_text(template),
    } == snapshot(
        {
            "data": {
                "Image": {
                    "Width": 800,
                    "Height": 600,
                    "Title": "View from 15th Floor",
                    "Thumbnail": {
                        "Url": "http://www.example.com/image/481989943",
                        "Height": 125,
                        "Width": 100,
                    },
                    "Animated": False,
                    "IDs": [116, 943, 234, 38793],
                }
            },
            "text": (
                '{"Image": {"Width": 800, "Height": 600, '
                '"Title": "View from 15th Floor", "Thumbnail": '
                '{"Url": "http://www.example.com/image/481989943", '
                '"Height": 125, "Width": 100}, "Animated": false, '
                '"IDs": [116, 943, 234, 38793]}}'
            ),
        }
    )


def test_json_rfc_8259_value_examples_round_trip() -> None:
    template = t"""[
      {{
         "precision": "zip",
         "Latitude":  37.7668,
         "Longitude": -122.3959,
         "Address":   "",
         "City":      "SAN FRANCISCO",
         "State":     "CA",
         "Zip":       "94107",
         "Country":   "US"
      }},
      {{
         "precision": "zip",
         "Latitude":  37.371991,
         "Longitude": -122.026020,
         "Address":   "",
         "City":      "SUNNYVALE",
         "State":     "CA",
         "Zip":       "94085",
         "Country":   "US"
      }}
    ]"""

    assert {
        "array": render_data(template),
        "string": render_data(t'"Hello world!"'),
        "number": render_data(t"42"),
        "boolean": render_data(t"true"),
        "text": render_text(template),
    } == snapshot(
        {
            "array": [
                {
                    "precision": "zip",
                    "Latitude": 37.7668,
                    "Longitude": -122.3959,
                    "Address": "",
                    "City": "SAN FRANCISCO",
                    "State": "CA",
                    "Zip": "94107",
                    "Country": "US",
                },
                {
                    "precision": "zip",
                    "Latitude": 37.371991,
                    "Longitude": -122.02602,
                    "Address": "",
                    "City": "SUNNYVALE",
                    "State": "CA",
                    "Zip": "94085",
                    "Country": "US",
                },
            ],
            "string": "Hello world!",
            "number": 42,
            "boolean": True,
            "text": (
                '[{"precision": "zip", "Latitude": 37.7668, '
                '"Longitude": -122.3959, "Address": "", '
                '"City": "SAN FRANCISCO", "State": "CA", '
                '"Zip": "94107", "Country": "US"}, '
                '{"precision": "zip", "Latitude": 37.371991, '
                '"Longitude": -122.026020, "Address": "", '
                '"City": "SUNNYVALE", "State": "CA", '
                '"Zip": "94085", "Country": "US"}]'
            ),
        }
    )


def test_json_numbers_and_escape_sequences_follow_rfc_8259() -> None:
    assert render_data(
        t"""
        {{
          "int": -0,
          "exp": 1.5e2,
          "escapes": "\\b\\f\\n\\r\\t\\/",
          "unicode": "\\u00DF\\u6771\\uD834\\uDD1E"
        }}
        """
    ) == snapshot(
        {
            "int": 0,
            "exp": 150.0,
            "escapes": "\b\f\n\r\t/",
            "unicode": "ß東𝄞",
        }
    )


def test_json_whitespace_and_escaped_solidus_cases_follow_rfc_8259() -> None:
    assert {
        "top_bool_ws": render_data(Template(" \n true \t ")),
        "top_null_ws": render_data(Template(" \r\n null \n")),
        "empty_string": render_data(t'""'),
        "empty_object": render_data(Template("{ \n\t }")),
        "empty_array": render_data(Template("[ \n\t ]")),
        "array_empty_values": render_data(Template('["", 0, false, null, {}, []]')),
        "empty_object_in_array": render_data(Template('[{}, {"a": []}]')),
        "top_level_empty_object_ws": render_data(Template(" \n { } \t ")),
        "escaped_controls": render_data(t'"\\b\\f\\n\\r\\t"'),
        "escaped_solidus": render_data(t'"\\/"'),
        "escaped_backslash": render_data(t'"\\\\"'),
        "unicode_backslash_escape": render_data(t'"\\u005C"'),
        "reverse_solidus_u": render_data(t'"\\u005C/"'),
        "escaped_quote_backslash": render_data(t'"\\"\\\\"'),
        "escaped_null_and_unit_separator": render_data(t'"\\u0000\\u001f"'),
        "nested_upper_unicode": render_data(t'"\\u00DF\\u6771"'),
        "unicode_line_sep": render_data(t'"\\u2028"'),
        "unicode_para_sep": render_data(t'"\\u2029"'),
        "array_with_line_sep": render_data(t'["\\u2028", "\\u2029"]'),
        "unicode_escapes_array": render_data(Template('["\\u005C", "\\/", "\\u00DF"]')),
        "unicode_mix_nested_obj": render_data(
            Template('{"x": {"a": "\\u005C", "b": "\\u00DF", "c": "\\u2029"}}')
        ),
        "nested_unicode_object_array": render_data(
            Template('{"a": [{"b": "\\u005C", "c": "\\u00DF"}]}')
        ),
        "escaped_slash_backslash_quote": render_data(t'"\\/\\\\\\""'),
        "escaped_reverse_solidus_solidus": render_data(t'"\\\\/"'),
        "nested_escaped_mix": render_data(Template('{"x":"\\b\\u2028\\u2029\\/"}')),
        "upper_exp": (repr(render_data(t"1E2")), type(render_data(t"1E2")).__name__),
        "upper_exp_plus": (
            repr(render_data(t"1E+2")),
            type(render_data(t"1E+2")).__name__,
        ),
        "upper_exp_negative": (
            repr(render_data(Template("-1E+2"))),
            type(render_data(Template("-1E+2"))).__name__,
        ),
        "nested_upper_exp": render_data(Template('{"value": 1E+2}')),
        "neg_exp_zero": (
            repr(render_data(Template("-1e-0"))),
            type(render_data(Template("-1e-0"))).__name__,
        ),
        "upper_exp_negative_zero": (
            repr(render_data(Template("1E-0"))),
            type(render_data(Template("1E-0"))).__name__,
        ),
        "exp_with_fraction_zero": (
            repr(render_data(Template("1.0e-0"))),
            type(render_data(Template("1.0e-0"))).__name__,
        ),
        "negative_zero_exp_upper": (
            repr(render_data(t"-0E0")),
            type(render_data(t"-0E0")).__name__,
        ),
        "nested_negative_exp_mix": render_data(
            Template('{"x":[-1E-2,0,"",{"y":[null]}]}')
        ),
        "mixed_nested_keywords": render_data(
            Template('{"a": [true, false, null], "b": {"c": -1e-0}}')
        ),
        "nested_bool_null_mix": render_data(
            Template('{"v": [true, null, false, {"x": 1}]}')
        ),
        "keyword_array": render_data(Template("[true,false,null]")),
        "empty_name_nested_keywords": render_data(
            Template('{"": [null, true, false]}')
        ),
        "nested_empty_mix": render_data(
            Template('{"a": [{}, [], "", 0, false, null]}')
        ),
        "array_nested_mixed_scalars": render_data(
            Template('[{"a": []}, {"b": {}}, "", 0, false, null]')
        ),
        "nested_empty_collections_mix": render_data(
            Template('{"a": {"b": []}, "c": [{}, []]}')
        ),
        "nested_number_combo": render_data(
            Template('{"a": [0, -0, -0.0, 1e0, -1E-0]}')
        ),
        "nested_empty_names": render_data(Template('{"": {"": []}}')),
        "nested_empty_name_array": render_data(Template('{"": ["", {"": 0}]}')),
        "nested_nulls": render_data(Template('{"a": null, "b": [null, {"c": null}]}')),
        "nested_top_ws": render_data(
            Template('\r\n {"a": [1, {"b": "c"}], "": ""} \n')
        ),
        "nested_number_whitespace": render_data(
            Template('{"a": [ 0 , -0 , 1.5E-2 ] }')
        ),
        "nested": render_data(Template('[\n {"a": 1, "b": [true, false, null]}\n]')),
        "top_ws_string": render_data(Template('\n\r\t "x" \n')),
        "upper_unicode_mix_array": render_data(
            Template('["\\u00DF", "\\u6771", "\\u2028"]')
        ),
        "upper_exp_zero_fraction": (
            repr(render_data(t"0E+0")),
            type(render_data(t"0E+0")).__name__,
        ),
        "upper_zero_negative_exp": (
            repr(render_data(t"-0E-0")),
            type(render_data(t"-0E-0")).__name__,
        ),
        "zero_fraction_exp": (
            repr(render_data(t"0.0e+0")),
            type(render_data(t"0.0e+0")).__name__,
        ),
    } == snapshot(
        {
            "top_bool_ws": True,
            "top_null_ws": None,
            "empty_string": "",
            "empty_object": {},
            "empty_array": [],
            "array_empty_values": ["", 0, False, None, {}, []],
            "empty_object_in_array": [{}, {"a": []}],
            "top_level_empty_object_ws": {},
            "escaped_controls": "\b\f\n\r\t",
            "escaped_solidus": "/",
            "escaped_backslash": "\\",
            "unicode_backslash_escape": "\\",
            "reverse_solidus_u": "\\/",
            "escaped_quote_backslash": '"\\',
            "escaped_null_and_unit_separator": "\x00\x1f",
            "nested_upper_unicode": "ß東",
            "unicode_line_sep": "\u2028",
            "unicode_para_sep": "\u2029",
            "array_with_line_sep": ["\u2028", "\u2029"],
            "unicode_escapes_array": ["\\", "/", "ß"],
            "unicode_mix_nested_obj": {"x": {"a": "\\", "b": "ß", "c": "\u2029"}},
            "nested_unicode_object_array": {"a": [{"b": "\\", "c": "ß"}]},
            "escaped_slash_backslash_quote": '/\\"',
            "escaped_reverse_solidus_solidus": "\\/",
            "nested_escaped_mix": {"x": "\b\u2028\u2029/"},
            "upper_exp": ("100.0", "float"),
            "upper_exp_plus": ("100.0", "float"),
            "upper_exp_negative": ("-100.0", "float"),
            "nested_upper_exp": {"value": 100.0},
            "neg_exp_zero": ("-1.0", "float"),
            "upper_exp_negative_zero": ("1.0", "float"),
            "exp_with_fraction_zero": ("1.0", "float"),
            "upper_zero_negative_exp": ("-0.0", "float"),
            "negative_zero_exp_upper": ("-0.0", "float"),
            "nested_bool_null_mix": {"v": [True, None, False, {"x": 1}]},
            "nested_negative_exp_mix": {"x": [-0.01, 0, "", {"y": [None]}]},
            "mixed_nested_keywords": {"a": [True, False, None], "b": {"c": -1.0}},
            "keyword_array": [True, False, None],
            "empty_name_nested_keywords": {"": [None, True, False]},
            "nested_empty_mix": {"a": [{}, [], "", 0, False, None]},
            "array_nested_mixed_scalars": [{"a": []}, {"b": {}}, "", 0, False, None],
            "nested_empty_collections_mix": {"a": {"b": []}, "c": [{}, []]},
            "nested_number_combo": {"a": [0, 0, -0.0, 1.0, -1.0]},
            "nested_empty_names": {"": {"": []}},
            "nested_empty_name_array": {"": ["", {"": 0}]},
            "nested_nulls": {"a": None, "b": [None, {"c": None}]},
            "nested_top_ws": {"a": [1, {"b": "c"}], "": ""},
            "nested_number_whitespace": {"a": [0, 0, 0.015]},
            "nested": [{"a": 1, "b": [True, False, None]}],
            "top_ws_string": "x",
            "upper_unicode_mix_array": ["ß", "東", "\u2028"],
            "upper_exp_zero_fraction": ("0.0", "float"),
            "zero_fraction_exp": ("0.0", "float"),
        }
    )


def test_json_nested_render_in_string_fragment_is_safe() -> None:
    fragment = NestedJsonFragment()

    assert render_data(t'{{"value": "{fragment}"}}') == {"value": "inner"}


def test_json_preserves_exact_large_python_integers() -> None:
    value = 2**100
    nested = {"positive": value, "negative": -value, "items": [value, -value]}

    assert render_data(t"{nested}") == nested
    assert render_data(t"{value}") == value
    assert render_text(t"{value}") == str(value)
    assert render_data(Template(str(value))) == value


def test_json_negative_zero_matches_python_json_semantics() -> None:
    top = render_data(t"-0")
    nested = _expect_json_object(render_data(Template('{"value": -0}')))
    exp = render_data(t"-0e0")
    float_value = render_data(t"-0.0")

    assert {
        "top": (repr(top), type(top).__name__),
        "nested": (repr(nested["value"]), type(nested["value"]).__name__),
        "exp": (repr(exp), type(exp).__name__),
        "float": (repr(float_value), type(float_value).__name__),
    } == snapshot(
        {
            "top": ("0", "int"),
            "nested": ("0", "int"),
            "exp": ("-0.0", "float"),
            "float": ("-0.0", "float"),
        }
    )


def test_json_core_helpers_are_available() -> None:
    value = "Alice"
    template = t'{{"name": {value}}}'
    tokens = tokenize_template(template)
    items = flatten_template(template)
    span = SourceSpan.between(SourcePosition(0, 0), SourcePosition(0, 3))
    diagnostic = Diagnostic(code="demo", message="message", span=span)

    assert {
        "tokens": [type(token).__name__ for token in tokens],
        "first_token": isinstance(tokens[0], StaticTextToken),
        "items": [item.kind for item in items[:5]],
        "span_end": span.extend(SourcePosition(1, 2)).end.offset,
        "merged": span.merge(SourceSpan.point(2, 0)).end.token_index,
        "diagnostic": (diagnostic.code, diagnostic.severity),
        "slot_context": SlotContext.VALUE.value,
        "fragment_group": FragmentGroup(0, 1, 0, 2).end_slot,
        "slots_module": _slots.SlotContext.STRING_FRAGMENT.value,
    } == snapshot(
        {
            "tokens": ["StaticTextToken", "InterpolationToken", "StaticTextToken"],
            "first_token": True,
            "items": ["char", "char", "char", "char", "char"],
            "span_end": 2,
            "merged": 2,
            "diagnostic": ("demo", DiagnosticSeverity.ERROR),
            "slot_context": "value",
            "fragment_group": 1,
            "slots_module": "string_fragment",
        }
    )


def test_json_requires_a_template_object() -> None:
    with pytest.raises(
        TypeError,
        match="render_data requires a PEP 750 Template object",
    ):
        render_data("not-a-template")  # type: ignore[arg-type]

    with pytest.raises(
        TypeError,
        match="render_text requires a PEP 750 Template object",
    ):
        render_text("not-a-template")  # type: ignore[arg-type]

    with pytest.raises(
        TypeError,
        match="render_result requires a PEP 750 Template object",
    ):
        render_result("not-a-template")  # type: ignore[arg-type]


def test_json_parse_errors_are_structural() -> None:
    with pytest.raises(TemplateParseError, match="JSON object keys"):
        render_text(t"{{name: 1}}")

    with pytest.raises(TemplateParseError, match="Unterminated JSON string"):
        render_text(t'{{"name": "alice}}')

    with pytest.raises(TemplateParseError, match="Invalid JSON number literal"):
        render_text(t"01")

    with pytest.raises(TemplateParseError, match="Invalid JSON escape sequence"):
        render_text(Template('"\\x41"'))

    with pytest.raises(TemplateParseError, match="Invalid JSON unicode escape"):
        render_text(Template('"\\uZZZZ"'))

    with pytest.raises(
        TemplateParseError, match="Unexpected end of JSON escape sequence"
    ):
        render_text(Template('"\\u12"'))

    with pytest.raises(TemplateParseError, match="Control characters are not allowed"):
        render_text(Template('"a\nb"'))

    with pytest.raises(TemplateParseError, match="Invalid JSON unicode escape"):
        render_text(Template('"\\uDD1E"'))

    with pytest.raises(TemplateParseError, match="Invalid JSON number literal"):
        render_text(Template("1e+"))

    with pytest.raises(TemplateParseError, match="Invalid JSON number literal"):
        render_text(Template("1e"))

    with pytest.raises(TemplateParseError, match="Invalid JSON number literal"):
        render_text(Template("1."))

    with pytest.raises(TemplateParseError, match="Invalid JSON number literal"):
        render_text(Template("-"))

    with pytest.raises(TemplateParseError, match="Invalid JSON number literal"):
        render_text(Template('{"a": 1e+}'))

    with pytest.raises(TemplateParseError, match="Invalid JSON number literal"):
        render_text(Template('{"a": 1.}'))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template("true false"))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template("[1,,2]"))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template("[,1]"))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template('{"a":,1}'))

    with pytest.raises(TemplateParseError, match="quoted strings or interpolations"):
        render_text(Template("{,}"))

    with pytest.raises(TemplateParseError, match="Expected ':' in JSON template"):
        render_text(Template('{"a" 1}'))

    with pytest.raises(TemplateParseError, match="quoted strings or interpolations"):
        render_text(Template('{"a":1,}'))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template("[1,2,]"))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template("+1"))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template(".1"))

    with pytest.raises(TemplateParseError, match="Invalid JSON number literal"):
        render_text(Template("00"))

    with pytest.raises(TemplateParseError, match="Invalid JSON number literal"):
        render_text(Template("-01"))

    with pytest.raises(TemplateParseError, match="Invalid JSON number literal"):
        render_text(Template('{"a": -01}'))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template("[true false]"))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template('{"a": true false}'))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template("tru"))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template("fals"))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template("nul"))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template('{"a": tru}'))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template('{"a": fals}'))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template('{"a": nul}'))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template("[nul]"))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template("[tru]"))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template("[fals]"))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template('{"a": [fals]}'))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template('{"a": [tru]}'))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template('[{"a": nul}]'))

    with pytest.raises(TemplateParseError, match="Expected ',' in JSON template"):
        render_text(Template('["a" true]'))

    with pytest.raises(TemplateParseError, match="Invalid JSON number literal"):
        render_text(Template('{"a": [1 2]}'))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template('{"a": [true false]}'))

    with pytest.raises(TemplateParseError, match="Expected ',' in JSON template"):
        render_text(Template('[{"a": 1} {"b": 2}]'))

    with pytest.raises(
        TemplateParseError, match="Invalid promoted JSON fragment content"
    ):
        render_text(Template('[null {"a":1}]'))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template("truE"))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template("[truE]"))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template("falsE"))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template("nulL"))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template('{"a": truE}'))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template('{"a": falsE}'))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template('{"a": nulL}'))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template("[falsE]"))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template("[nulL]"))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template('{"a": [nulL]}'))

    with pytest.raises(TemplateParseError, match="Invalid JSON number literal"):
        render_text(Template("01e0"))

    with pytest.raises(TemplateParseError, match="Invalid JSON number literal"):
        render_text(Template("-01e0"))

    with pytest.raises(TemplateParseError, match="Invalid JSON number literal"):
        render_text(Template("1e-"))

    with pytest.raises(TemplateParseError, match="Invalid JSON number literal"):
        render_text(Template('{"x": 1e-}'))

    with pytest.raises(TemplateParseError, match="Invalid JSON number literal"):
        render_text(Template("-+1"))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template("+-1"))

    with pytest.raises(TemplateParseError, match="Invalid JSON number literal"):
        render_text(Template("1e+-1"))

    with pytest.raises(TemplateParseError, match="Invalid JSON number literal"):
        render_text(Template("1e-+1"))

    with pytest.raises(TemplateParseError, match="Expected ',' in JSON template"):
        render_text(Template('["a" true]'))

    with pytest.raises(
        TemplateParseError, match="Invalid promoted JSON fragment content"
    ):
        render_text(Template('{"a": null "b": 1}'))

    with pytest.raises(TemplateParseError, match="Invalid JSON number literal"):
        render_text(Template("1.2.3"))

    with pytest.raises(TemplateParseError, match="Invalid JSON number literal"):
        render_text(Template('{"a": 00}'))

    with pytest.raises(
        TemplateParseError, match="Unexpected trailing content in JSON template"
    ):
        render_text(Template('"a" "b"'))

    with pytest.raises(
        TemplateParseError, match="Unexpected trailing content in JSON template"
    ):
        render_text(Template("[1]]"))

    with pytest.raises(
        TemplateParseError, match="Unexpected trailing content in JSON template"
    ):
        render_text(Template('{"a":1}}'))

    with pytest.raises(TemplateParseError, match="Invalid JSON number literal"):
        render_text(Template("[1 2]"))

    with pytest.raises(TemplateParseError, match="Invalid JSON number literal"):
        render_text(Template('{"a":1 "b":2}'))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template("[truee]"))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template("[true 1]"))

    with pytest.raises(TemplateParseError, match="Expected a JSON value"):
        render_text(Template('{"a": true 1}'))

    with pytest.raises(
        TemplateParseError, match="Invalid promoted JSON fragment content"
    ):
        render_text(Template('[false {"a":1}]'))

    with pytest.raises(TemplateParseError, match="quoted strings or interpolations"):
        render_text(Template('[{"a":1,}]'))

    with pytest.raises(
        TemplateParseError, match="Unexpected trailing content in JSON template"
    ):
        render_text(Template('{"a": [1]}]'))

    with pytest.raises(
        TemplateParseError, match="Unexpected trailing content in JSON template"
    ):
        render_text(Template('[{"a":1}]]'))

    with pytest.raises(
        TemplateParseError, match="Unexpected trailing content in JSON template"
    ):
        render_text(Template('{"a": {"b": 1}}}'))


def test_json_unrepresentable_values_fail() -> None:
    bad_key = 3
    bad_mapping = {1: "x"}
    bad_value = {1, 2}

    with pytest.raises(UnrepresentableValueError, match="object key"):
        render_text(t"{{{bad_key}: 1}}")

    with pytest.raises(UnrepresentableValueError, match="object key"):
        render_data(t'{{"payload": {bad_mapping}}}')

    with pytest.raises(UnrepresentableValueError, match="non-finite float"):
        render_text(t'{{"ratio": {float("inf")}}}')

    with pytest.raises(UnrepresentableValueError, match="set"):
        render_text(t'{{"items": {bad_value}}}')


def test_json_fragment_stringification_errors_surface_cleanly() -> None:
    bad = BadStringValue()

    with pytest.raises(UnrepresentableValueError, match="string fragment"):
        render_text(t'{{"name": "prefix-{bad}"}}')


def test_json_single_runtime_path_is_not_loose_mode() -> None:
    name = "Alice"

    assert {
        "text": render_text(t'{{"name": {name}}}'),
        "data": render_data(t'{{"name": {name}}}'),
    } == snapshot(
        {
            "text": '{"name": "Alice"}',
            "data": {"name": "Alice"},
        }
    )

    with pytest.raises(TemplateParseError, match="JSON object keys"):
        render_text(t"{{name: 1}}")


def test_json_render_result_matches_render_data_and_render_text() -> None:
    name = "Alice"
    template = t'{{"name": {name}}}'

    result = render_result(template)
    assert isinstance(result, RenderResult)
    assert result.data == render_data(template)
    assert result.text == render_text(template)


def test_json_template_error_base_class_can_be_instantiated() -> None:
    error = TemplateError("message")
    assert str(error) == snapshot("message")


def test_json_core_error_classes_are_exposed() -> None:
    assert {
        "parse": issubclass(TemplateParseError, TemplateError),
        "semantic": issubclass(TemplateSemanticError, TemplateError),
        "unrepr": issubclass(UnrepresentableValueError, TemplateError),
    } == snapshot({"parse": True, "semantic": True, "unrepr": True})


def test_json_public_exports_are_standardized() -> None:
    assert {
        "all": json_tstring.__all__,
        "parse_identity": json_tstring.TemplateParseError is TemplateParseError,
        "semantic_identity": json_tstring.TemplateSemanticError
        is TemplateSemanticError,
        "unrepr_identity": json_tstring.UnrepresentableValueError
        is UnrepresentableValueError,
        "render_result_type_identity": json_tstring.RenderResult is CoreRenderResult,
        "imported_render_result_type_identity": RenderResult is CoreRenderResult,
        "template_error_identity": json_tstring.TemplateError is TemplateError,
        "render_data_identity": json_tstring.render_data is render_data,
        "render_result_identity": json_tstring.render_result is render_result,
        "render_text_identity": json_tstring.render_text is render_text,
    } == snapshot(
        {
            "all": [
                "JsonProfile",
                "RenderResult",
                "TemplateError",
                "TemplateParseError",
                "TemplateSemanticError",
                "UnrepresentableValueError",
                "render_data",
                "render_result",
                "render_text",
            ],
            "parse_identity": True,
            "semantic_identity": True,
            "unrepr_identity": True,
            "render_result_type_identity": True,
            "imported_render_result_type_identity": True,
            "template_error_identity": True,
            "render_data_identity": True,
            "render_result_identity": True,
            "render_text_identity": True,
        }
    )
