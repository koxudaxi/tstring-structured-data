from __future__ import annotations

import math
from datetime import UTC, date, datetime, time, timedelta, timezone
from string.templatelib import Template
from typing import TypeIs

import pytest
from inline_snapshot import snapshot
from tstring_core import (
    Diagnostic,
    DiagnosticSeverity,
    SourceSpan,
    TemplateError,
    TemplateParseError,
    TemplateSemanticError,
    TomlValue,
    UnrepresentableValueError,
    tokenize_template,
)
from tstring_core import (
    RenderResult as CoreRenderResult,
)

import toml_tstring
from toml_tstring import RenderResult, _slots, render_data, render_result, render_text

type TomlTable = dict[str, TomlValue]
type TomlArray = list[TomlValue]


def _is_toml_table(value: TomlValue) -> TypeIs[TomlTable]:
    return isinstance(value, dict)


def _is_toml_array(value: TomlValue) -> TypeIs[TomlArray]:
    return isinstance(value, list)


def _expect_toml_table(value: TomlValue) -> TomlTable:
    assert _is_toml_table(value)
    return value


def _expect_toml_array(value: TomlValue) -> TomlArray:
    assert _is_toml_array(value)
    return value


def _expect_toml_string(value: TomlValue) -> str:
    assert isinstance(value, str)
    return value


def _expect_toml_float(value: TomlValue) -> float:
    assert isinstance(value, float)
    return value


class BadStringValue:
    def __str__(self) -> str:
        raise ValueError("cannot stringify")


class NestedTomlFragment:
    def __str__(self) -> str:
        inner = "inner"
        return _expect_toml_string(
            _expect_toml_table(render_data(t'value = "{inner}"'))["value"]
        )


def test_toml_cache_reuses_static_structure_across_runtime_values() -> None:
    first_value = "Alice"
    second_value = "Bob"

    assert render_text(t"value = {first_value}") == 'value = "Alice"'
    assert render_text(t"value = {second_value}") == 'value = "Bob"'


def test_toml_end_to_end_supported_positions() -> None:
    key = "leaf"
    left = "prefix"
    right = "suffix"
    created = datetime(2024, 1, 2, 3, 4, 5, tzinfo=UTC)

    template = t"""
    title = "item-{left}"
    [root.{key}]
    name = {right}
    label = "{left}-{right}"
    created = {created}
    rows = [{left}, {right}]
    meta = {{ enabled = true, target = {right} }}
    """

    assert {
        "data": render_data(template),
        "text": render_text(template),
    } == snapshot(
        {
            "data": {
                "title": "item-prefix",
                "root": {
                    "leaf": {
                        "name": "suffix",
                        "label": "prefix-suffix",
                        "created": datetime(2024, 1, 2, 3, 4, 5, tzinfo=UTC),
                        "rows": ["prefix", "suffix"],
                        "meta": {"enabled": True, "target": "suffix"},
                    }
                },
            },
            "text": (
                'title = "item-prefix"\n[root."leaf"]\nname = "suffix"\n'
                'label = "prefix-suffix"\ncreated = 2024-01-02T03:04:05+00:00\n'
                'rows = ["prefix", "suffix"]\n'
                'meta = { enabled = true, target = "suffix" }'
            ),
        }
    )


def test_toml_string_families_round_trip() -> None:
    value = "name"

    assert {
        "basic": render_data(t'basic = "hi-{value}"'),
        "literal": render_data(t"literal = 'hi-{value}'"),
        "multi_basic": render_data(t'multi_basic = """hi-{value}"""'),
        "multi_literal": render_data(t"""multi_literal = '''hi-{value}'''"""),
    } == snapshot(
        {
            "basic": {"basic": "hi-name"},
            "literal": {"literal": "hi-name"},
            "multi_basic": {"multi_basic": "hi-name"},
            "multi_literal": {"multi_literal": "hi-name"},
        }
    )


def test_toml_array_of_tables_and_comments_round_trip() -> None:
    name = "api"
    worker = "worker"

    template = t"""
    # comment before content
    [[services]]
    name = {name} # inline comment

    [[services]]
    name = {worker}
    """

    assert render_data(template) == snapshot(
        {"services": [{"name": "api"}, {"name": "worker"}]}
    )


def test_toml_array_of_tables_spec_example_round_trip() -> None:
    template = t"""
    [[products]]
    name = "Hammer"
    sku = 738594937

    [[products]]
    name = "Nail"
    sku = 284758393
    color = "gray"
    """

    assert {
        "data": render_data(template),
        "text": render_text(template),
    } == snapshot(
        {
            "data": {
                "products": [
                    {"name": "Hammer", "sku": 738594937},
                    {"name": "Nail", "sku": 284758393, "color": "gray"},
                ]
            },
            "text": (
                '[[products]]\nname = "Hammer"\nsku = 738594937\n'
                '[[products]]\nname = "Nail"\nsku = 284758393\ncolor = "gray"'
            ),
        }
    )


def test_toml_nested_array_of_tables_spec_hierarchy_round_trip() -> None:
    template = t"""
    [[fruit]]
    name = "apple"

    [fruit.physical]
    color = "red"
    shape = "round"

    [[fruit.variety]]
    name = "red delicious"

    [[fruit.variety]]
    name = "granny smith"

    [[fruit]]
    name = "banana"

    [[fruit.variety]]
    name = "plantain"
    """

    assert {
        "data": render_data(template),
        "text": render_text(template),
    } == snapshot(
        {
            "data": {
                "fruit": [
                    {
                        "name": "apple",
                        "physical": {"color": "red", "shape": "round"},
                        "variety": [
                            {"name": "red delicious"},
                            {"name": "granny smith"},
                        ],
                    },
                    {"name": "banana", "variety": [{"name": "plantain"}]},
                ]
            },
            "text": (
                '[[fruit]]\nname = "apple"\n[fruit.physical]\ncolor = "red"\n'
                'shape = "round"\n[[fruit.variety]]\nname = "red delicious"\n'
                '[[fruit.variety]]\nname = "granny smith"\n[[fruit]]\n'
                'name = "banana"\n[[fruit.variety]]\nname = "plantain"'
            ),
        }
    )


def test_toml_main_spec_example_round_trip() -> None:
    template = t"""
    title = "TOML Example"

    [owner]
    name = "Tom Preston-Werner"
    dob = 1979-05-27T07:32:00-08:00

    [database]
    enabled = true
    ports = [ 8000, 8001, 8002 ]
    data = [ ["delta", "phi"], [3.14] ]
    temp_targets = {{ cpu = 79.5, case = 72.0 }}

    [servers]

    [servers.alpha]
    ip = "10.0.0.1"
    role = "frontend"

    [servers.beta]
    ip = "10.0.0.2"
    role = "backend"
    """

    assert {
        "data": render_data(template),
        "text": render_text(template),
    } == snapshot(
        {
            "data": {
                "title": "TOML Example",
                "owner": {
                    "name": "Tom Preston-Werner",
                    "dob": datetime(
                        1979,
                        5,
                        27,
                        7,
                        32,
                        tzinfo=timezone(timedelta(hours=-8)),
                    ),
                },
                "database": {
                    "enabled": True,
                    "ports": [8000, 8001, 8002],
                    "data": [["delta", "phi"], [3.14]],
                    "temp_targets": {"cpu": 79.5, "case": 72.0},
                },
                "servers": {
                    "alpha": {"ip": "10.0.0.1", "role": "frontend"},
                    "beta": {"ip": "10.0.0.2", "role": "backend"},
                },
            },
            "text": (
                'title = "TOML Example"\n'
                "[owner]\n"
                'name = "Tom Preston-Werner"\n'
                "dob = 1979-05-27T07:32:00-08:00\n"
                "[database]\n"
                "enabled = true\n"
                "ports = [8000, 8001, 8002]\n"
                'data = [["delta", "phi"], [3.14]]\n'
                "temp_targets = { cpu = 79.5, case = 72.0 }\n"
                "[servers]\n"
                "[servers.alpha]\n"
                'ip = "10.0.0.1"\n'
                'role = "frontend"\n'
                "[servers.beta]\n"
                'ip = "10.0.0.2"\n'
                'role = "backend"'
            ),
        }
    )


def test_toml_quoted_keys_and_multiline_array_comments_round_trip() -> None:
    assert {
        "quoted": render_data(
            t"""
            "a.b" = 1
            site."google.com".value = 2
            value = [
              1, # first
              2, # second
            ]
            """
        ),
        "empty_basic": render_data(Template('"" = 1\n')),
        "empty_literal": render_data(Template("'' = 1\n")),
        "empty_segment": render_data(Template('a."" .b = 1\n'.replace(" ", ""))),
    } == snapshot(
        {
            "quoted": {
                "a.b": 1,
                "site": {"google.com": {"value": 2}},
                "value": [1, 2],
            },
            "empty_basic": {"": 1},
            "empty_literal": {"": 1},
            "empty_segment": {"a": {"": {"b": 1}}},
        }
    )


def test_toml_multiline_basic_strings_follow_spec_trimming() -> None:
    assert render_data(t'value = """\nalpha\\\n  beta\n"""') == snapshot(
        {"value": "alphabeta\n"}
    )

    assert render_data(Template('value = """\r\na\\\r\n  b\r\n"""\n')) == snapshot(
        {"value": "ab\n"}
    )


def test_toml_multiline_strings_allow_one_or_two_quotes_before_terminator() -> None:
    assert {
        "basic_one": render_data(t'value = """""""\n'),
        "basic_two": render_data(t'value = """"""""\n'),
        "basic_escaped_quote": render_data(t'value = """a\\"b"""\n'),
        "literal_one": render_data(t"""value = '''''''\n"""),
        "literal_two": render_data(t"""value = ''''''''\n"""),
    } == snapshot(
        {
            "basic_one": {"value": '"'},
            "basic_two": {"value": '""'},
            "basic_escaped_quote": {"value": 'a"b'},
            "literal_one": {"value": "'"},
            "literal_two": {"value": "''"},
        }
    )


def test_toml_empty_strings_and_quoted_empty_table_headers_round_trip() -> None:
    assert {
        "basic": render_data(t'value = ""\n'),
        "literal": render_data(Template("value = ''\n")),
        "header": render_data(Template('[""]\nvalue = 1\n')),
        "header_subtable": render_data(
            Template('[""]\nvalue = 1\n["".inner]\nname = "x"\n')
        ),
        "quoted_header_segments": render_data(Template('["a"."b"]\nvalue = 1\n')),
    } == snapshot(
        {
            "basic": {"value": ""},
            "literal": {"value": ""},
            "header": {"": {"value": 1}},
            "header_subtable": {"": {"value": 1, "inner": {"name": "x"}}},
            "quoted_header_segments": {"a": {"b": {"value": 1}}},
        }
    )


def test_toml_empty_collections_and_quoted_empty_dotted_tables_round_trip() -> None:
    assert {
        "empty_array": render_data(Template("value = []\n")),
        "empty_inline_table": render_data(Template("value = {}\n")),
        "quoted_empty_dotted_table": render_data(Template('[a."".b]\nvalue = 1\n')),
        "quoted_empty_subsegments": render_data(Template('[""."".leaf]\nvalue = 1\n')),
        "quoted_empty_and_named": render_data(
            Template('["".leaf."node"]\nvalue = 1\n')
        ),
        "quoted_empty_leaf_chain": render_data(Template('["".""."leaf"]\nvalue = 1\n')),
        "mixed_array_tables": render_data(
            Template('[[a]]\nname = "x"\n[[a]]\nname = "y"\n')
        ),
    } == snapshot(
        {
            "empty_array": {"value": []},
            "empty_inline_table": {"value": {}},
            "quoted_empty_dotted_table": {"a": {"": {"b": {"value": 1}}}},
            "quoted_empty_subsegments": {"": {"": {"leaf": {"value": 1}}}},
            "quoted_empty_and_named": {"": {"leaf": {"node": {"value": 1}}}},
            "quoted_empty_leaf_chain": {"": {"": {"leaf": {"value": 1}}}},
            "mixed_array_tables": {"a": [{"name": "x"}, {"name": "y"}]},
        }
    )


def test_toml_numeric_and_local_datetime_forms_follow_toml_1_0() -> None:
    assert render_data(
        t"""
        value = 0xDEADBEEF
        hex_underscore = 0xDEAD_BEEF
        binary = 0b1101
        octal = 0o755
        underscored = 1_000_000
        float = +1.0
        exp = -2e-2
        local = 2024-01-02T03:04:05
        """
    ) == snapshot(
        {
            "value": 3735928559,
            "hex_underscore": 3735928559,
            "binary": 13,
            "octal": 493,
            "underscored": 1000000,
            "float": 1.0,
            "exp": -0.02,
            "local": datetime(2024, 1, 2, 3, 4, 5),
        }
    )


def test_toml_nested_render_in_string_fragment_is_safe() -> None:
    value = NestedTomlFragment()

    assert render_data(t'value = "{value}"') == {"value": "inner"}


def test_toml_rejects_out_of_range_integers_early() -> None:
    value = 2**63

    with pytest.raises(UnrepresentableValueError, match="signed 64-bit range"):
        render_data(t"value = {value}")

    with pytest.raises(UnrepresentableValueError, match="signed 64-bit range"):
        render_text(t"value = {value}")


def test_toml_additional_numeric_and_datetime_forms_follow_toml_1_0() -> None:
    actual = _expect_toml_table(
        render_data(
            t"""
        plus_int = +1
        plus_zero = +0
        plus_zero_float = +0.0
        zero_float_exp = 0e0
        plus_zero_float_exp = +0e0
        plus_zero_fraction_exp = +0.0e0
        exp_underscore = 1e1_0
        frac_underscore = 1_2.3_4
        local_space = 2024-01-02 03:04:05
        local_lower_t = 2024-01-02t03:04:05
        local_date = 2024-01-02
        local_time_fraction = 03:04:05.123456
        array_of_dates = [2024-01-02, 2024-01-03]
        array_of_dates_trailing = [2024-01-02, 2024-01-03,]
        mixed_date_time_array = [2024-01-02, 03:04:05]
        array_of_local_times = [03:04:05, 03:04:06.123456]
        nested_array_mixed_dates = [[2024-01-02], [2024-01-03]]
        offset_array = [1979-05-27T07:32:00Z, 1979-05-27T00:32:00-07:00]
        offset_array_positive = [1979-05-27T07:32:00+07:00]
        datetime_array_trailing = [1979-05-27T07:32:00Z, 1979-05-27T00:32:00-07:00,]
        offset_fraction_dt = 1979-05-27T07:32:00.999999-07:00
        offset_fraction_space = 1979-05-27 07:32:00.999999-07:00
        array_offset_fraction = [1979-05-27T07:32:00.999999-07:00, 1979-05-27T07:32:00Z]
        fraction_lower_z = 2024-01-02T03:04:05.123456z
        array_fraction_lower_z = [2024-01-02T03:04:05.123456z]
        utc_fraction_lower_array = [2024-01-02T03:04:05.123456z, 2024-01-02T03:04:06z]
        utc_fraction_lower_array_trailing = [
            2024-01-02T03:04:05.123456z,
            2024-01-02T03:04:06z,
        ]
        lowercase_offset_array_trailing = [2024-01-02T03:04:05z, 2024-01-02T03:04:06z,]
        lower_hex = 0xdeadbeef
        utc_z = 2024-01-02T03:04:05Z
        utc_lower_z = 2024-01-02T03:04:05z
        utc_fraction = 2024-01-02T03:04:05.123456Z
        utc_fraction_array = [2024-01-02T03:04:05.123456Z, 2024-01-02T03:04:06Z]
        upper_exp = 1E2
        signed_int_array = [+1, +0, -1]
        special_float_array = [+inf, -inf, nan]
        """
        )
    )

    normalized = {
        **actual,
        "special_float_array": [
            repr(value) for value in _expect_toml_array(actual["special_float_array"])
        ],
    }

    assert normalized == snapshot(
        {
            "plus_int": 1,
            "plus_zero": 0,
            "plus_zero_float": 0.0,
            "zero_float_exp": 0.0,
            "plus_zero_float_exp": 0.0,
            "plus_zero_fraction_exp": 0.0,
            "exp_underscore": 10000000000.0,
            "frac_underscore": 12.34,
            "local_space": datetime(2024, 1, 2, 3, 4, 5),
            "local_lower_t": datetime(2024, 1, 2, 3, 4, 5),
            "local_date": date(2024, 1, 2),
            "local_time_fraction": time(3, 4, 5, 123456),
            "array_of_dates": [date(2024, 1, 2), date(2024, 1, 3)],
            "array_of_dates_trailing": [date(2024, 1, 2), date(2024, 1, 3)],
            "mixed_date_time_array": [date(2024, 1, 2), time(3, 4, 5)],
            "array_of_local_times": [time(3, 4, 5), time(3, 4, 6, 123456)],
            "nested_array_mixed_dates": [[date(2024, 1, 2)], [date(2024, 1, 3)]],
            "offset_array": [
                datetime(1979, 5, 27, 7, 32, 0, tzinfo=UTC),
                datetime(1979, 5, 27, 0, 32, 0, tzinfo=timezone(timedelta(hours=-7))),
            ],
            "offset_array_positive": [
                datetime(1979, 5, 27, 7, 32, 0, tzinfo=timezone(timedelta(hours=7)))
            ],
            "datetime_array_trailing": [
                datetime(1979, 5, 27, 7, 32, 0, tzinfo=UTC),
                datetime(1979, 5, 27, 0, 32, 0, tzinfo=timezone(timedelta(hours=-7))),
            ],
            "offset_fraction_dt": datetime(
                1979, 5, 27, 7, 32, 0, 999999, tzinfo=timezone(timedelta(hours=-7))
            ),
            "offset_fraction_space": datetime(
                1979, 5, 27, 7, 32, 0, 999999, tzinfo=timezone(timedelta(hours=-7))
            ),
            "array_offset_fraction": [
                datetime(
                    1979, 5, 27, 7, 32, 0, 999999, tzinfo=timezone(timedelta(hours=-7))
                ),
                datetime(1979, 5, 27, 7, 32, 0, tzinfo=UTC),
            ],
            "fraction_lower_z": datetime(2024, 1, 2, 3, 4, 5, 123456, tzinfo=UTC),
            "array_fraction_lower_z": [
                datetime(2024, 1, 2, 3, 4, 5, 123456, tzinfo=UTC)
            ],
            "utc_fraction_lower_array": [
                datetime(2024, 1, 2, 3, 4, 5, 123456, tzinfo=UTC),
                datetime(2024, 1, 2, 3, 4, 6, tzinfo=UTC),
            ],
            "utc_fraction_lower_array_trailing": [
                datetime(2024, 1, 2, 3, 4, 5, 123456, tzinfo=UTC),
                datetime(2024, 1, 2, 3, 4, 6, tzinfo=UTC),
            ],
            "lowercase_offset_array_trailing": [
                datetime(2024, 1, 2, 3, 4, 5, tzinfo=UTC),
                datetime(2024, 1, 2, 3, 4, 6, tzinfo=UTC),
            ],
            "lower_hex": 3735928559,
            "utc_z": datetime(2024, 1, 2, 3, 4, 5, tzinfo=UTC),
            "utc_lower_z": datetime(2024, 1, 2, 3, 4, 5, tzinfo=UTC),
            "utc_fraction": datetime(2024, 1, 2, 3, 4, 5, 123456, tzinfo=UTC),
            "utc_fraction_array": [
                datetime(2024, 1, 2, 3, 4, 5, 123456, tzinfo=UTC),
                datetime(2024, 1, 2, 3, 4, 6, tzinfo=UTC),
            ],
            "upper_exp": 100.0,
            "signed_int_array": [1, 0, -1],
            "special_float_array": ["inf", "-inf", "nan"],
        }
    )
    special_float_array = _expect_toml_array(actual["special_float_array"])
    assert _expect_toml_float(special_float_array[0]) == float("inf")
    assert _expect_toml_float(special_float_array[1]) == float("-inf")
    special_float_nan = _expect_toml_float(special_float_array[2])
    assert special_float_nan != special_float_nan


def test_toml_arrays_allow_trailing_commas() -> None:
    assert render_data(
        Template(
            """
            value = [1, 2,]
            nested = [[ ], [1, 2,],]
            empty_inline_tables = [{}, {}]
            nested_empty_inline_arrays = { inner = [[], [1]] }
            """
        )
    ) == snapshot(
        {
            "value": [1, 2],
            "nested": [[], [1, 2]],
            "empty_inline_tables": [{}, {}],
            "nested_empty_inline_arrays": {"inner": [[], [1]]},
        }
    )


def test_toml_nested_collections_and_array_tables_round_trip() -> None:
    assert render_data(
        Template(
            """
            matrix = [[1, 2], [3, 4]]
            meta = { inner = { value = 1 } }
            nested_inline_arrays = { items = [[1, 2], [3, 4]] }

            [[services]]
            name = "api"

            [[services]]
            name = "worker"
            """
        )
    ) == snapshot(
        {
            "matrix": [[1, 2], [3, 4]],
            "meta": {"inner": {"value": 1}},
            "nested_inline_arrays": {"items": [[1, 2], [3, 4]]},
            "services": [{"name": "api"}, {"name": "worker"}],
        }
    )


def test_toml_headers_comments_and_crlf_literal_strings_round_trip() -> None:
    assert {
        "quoted_header": render_data(
            t"""
            ["a.b"]
            value = 1
            """
        ),
        "dotted_quoted_header": render_data(
            t"""
            [site."google.com"]
            value = 1
            """
        ),
        "comment_after_inline_table": render_data(
            Template("value = { a = 1 } # comment\n")
        ),
        "commented_array": render_data(
            t"""
            value = [
              1,
              # comment
              2,
            ]
            """
        ),
        "literal_crlf": render_data(Template("value = '''a\r\nb'''\n")),
        "array_table_followed_by_table": render_data(
            t"""
            [[items]]
            name = "a"

            [tool]
            value = 1
            """
        ),
    } == snapshot(
        {
            "quoted_header": {"a.b": {"value": 1}},
            "dotted_quoted_header": {"site": {"google.com": {"value": 1}}},
            "comment_after_inline_table": {"value": {"a": 1}},
            "commented_array": {"value": [1, 2]},
            "literal_crlf": {"value": "a\nb"},
            "array_table_followed_by_table": {
                "items": [{"name": "a"}],
                "tool": {"value": 1},
            },
        }
    )


def test_toml_inline_tables_and_header_progressions_round_trip() -> None:
    assert {
        "empty_inline_table": render_data(Template("value = {}\n")),
        "nested_inline_table": render_data(
            Template("value = { inner = { value = 1 } }\n")
        ),
        "deep_nested_inline_table": render_data(
            Template("value = { inner = { deep = { value = 1 } } }\n")
        ),
        "array_of_inline_tables": render_data(
            Template("value = [{ a = 1 }, { a = 2 }]\n")
        ),
        "array_of_arrays_of_inline_tables": render_data(
            Template("value = [[{ a = 1 }], [{ a = 2 }]]\n")
        ),
        "array_table_then_dotted_assign": render_data(
            t"""
            [[a.b]]
            name = "x"
            a.c = 1
            """
        ),
        "table_then_array_table_same_root": render_data(
            t"""
            [a]
            value = 1

            [[a.b]]
            name = "x"
            """
        ),
        "quoted_header_then_dotted": render_data(
            t"""
            ["a.b"]
            value = 1

            ["a.b".c]
            name = "x"
            """
        ),
    } == snapshot(
        {
            "empty_inline_table": {"value": {}},
            "nested_inline_table": {"value": {"inner": {"value": 1}}},
            "deep_nested_inline_table": {"value": {"inner": {"deep": {"value": 1}}}},
            "array_of_inline_tables": {"value": [{"a": 1}, {"a": 2}]},
            "array_of_arrays_of_inline_tables": {"value": [[{"a": 1}], [{"a": 2}]]},
            "array_table_then_dotted_assign": {
                "a": {"b": [{"a": {"c": 1}, "name": "x"}]}
            },
            "table_then_array_table_same_root": {
                "a": {"b": [{"name": "x"}], "value": 1}
            },
            "quoted_header_then_dotted": {"a.b": {"c": {"name": "x"}, "value": 1}},
        }
    )


def test_toml_inline_tables_remain_single_line() -> None:
    with pytest.raises(TemplateParseError, match="Expected a TOML key segment"):
        render_text(Template("value = { a = 1,\n b = 2 }\n"), profile="1.0")


def test_toml_special_float_forms_follow_toml_1_0() -> None:
    template = (
        t"pos = {float('inf')}\nplus_inf = +inf\n"
        t"neg = {float('-inf')}\nvalue = {float('nan')}\n"
        t"plus_nan = +nan\nminus_nan = -nan\n"
        t"array = [+inf, -inf, nan]\n"
        t"nested = [[1E2], [+0.0E0], [-1E-2]]\n"
        t"special_float_deeper_arrays = [[[+inf]], [[-inf]], [[nan]]]\n"
        t"upper_exp_nested_mixed = [[1E2, 0E0], [-1E-2]]\n"
        t"special_float_inline_table = {{ pos = +inf, neg = -inf, nan = nan }}\n"
        t"special_float_mixed_nested = [[+inf, -inf], [nan]]\n"
        t"nested_datetime_arrays = [[1979-05-27 07:32:00+07:00], "
        t"[1979-05-27T00:32:00-07:00]]\n"
        t"positive_offset_scalar_space = 1979-05-27 07:32:00+07:00\n"
        t"positive_offset_array_space = "
        t"[1979-05-27 07:32:00+07:00, 1979-05-27T00:32:00-07:00]\n"
        t"utc_fraction_array = [2024-01-02T03:04:05.123456Z, 2024-01-02T03:04:06Z]\n"
        t"utc_fraction_lower_array = "
        t"[2024-01-02T03:04:05.123456z, 2024-01-02T03:04:06z]\n"
        t"utc_fraction_lower_array_trailing = "
        t"[2024-01-02T03:04:05.123456z, 2024-01-02T03:04:06z,]"
    )
    rendered = render_text(template)
    data = _expect_toml_table(render_data(template))

    assert rendered == snapshot(
        "pos = inf\nplus_inf = +inf\nneg = -inf\n"
        "value = nan\nplus_nan = +nan\nminus_nan = -nan\n"
        "array = [+inf, -inf, nan]\n"
        "nested = [[1E2], [+0.0E0], [-1E-2]]\n"
        "special_float_deeper_arrays = [[[+inf]], [[-inf]], [[nan]]]\n"
        "upper_exp_nested_mixed = [[1E2, 0E0], [-1E-2]]\n"
        "special_float_inline_table = { pos = +inf, neg = -inf, nan = nan }\n"
        "special_float_mixed_nested = [[+inf, -inf], [nan]]\n"
        "nested_datetime_arrays = [[1979-05-27 07:32:00+07:00], "
        "[1979-05-27T00:32:00-07:00]]\n"
        "positive_offset_scalar_space = 1979-05-27 07:32:00+07:00\n"
        "positive_offset_array_space = "
        "[1979-05-27 07:32:00+07:00, 1979-05-27T00:32:00-07:00]\n"
        "utc_fraction_array = [2024-01-02T03:04:05.123456Z, 2024-01-02T03:04:06Z]\n"
        "utc_fraction_lower_array = "
        "[2024-01-02T03:04:05.123456z, 2024-01-02T03:04:06z]\n"
        "utc_fraction_lower_array_trailing = "
        "[2024-01-02T03:04:05.123456z, 2024-01-02T03:04:06z]"
    )
    pos = _expect_toml_float(data["pos"])
    plus_inf = _expect_toml_float(data["plus_inf"])
    neg = _expect_toml_float(data["neg"])
    value = _expect_toml_float(data["value"])
    plus_nan = _expect_toml_float(data["plus_nan"])
    minus_nan = _expect_toml_float(data["minus_nan"])
    array = _expect_toml_array(data["array"])
    nested = _expect_toml_array(data["nested"])
    special_float_deeper_arrays = _expect_toml_array(
        data["special_float_deeper_arrays"]
    )
    upper_exp_nested_mixed = _expect_toml_array(data["upper_exp_nested_mixed"])
    special_float_inline_table = _expect_toml_table(data["special_float_inline_table"])
    special_float_mixed_nested = _expect_toml_array(data["special_float_mixed_nested"])
    nested_datetime_arrays = _expect_toml_array(data["nested_datetime_arrays"])
    positive_offset_array_space = _expect_toml_array(
        data["positive_offset_array_space"]
    )
    utc_fraction_array = _expect_toml_array(data["utc_fraction_array"])
    utc_fraction_lower_array = _expect_toml_array(data["utc_fraction_lower_array"])
    utc_fraction_lower_array_trailing = _expect_toml_array(
        data["utc_fraction_lower_array_trailing"]
    )

    assert math.isinf(pos) and pos > 0
    assert math.isinf(plus_inf) and plus_inf > 0
    assert math.isinf(neg) and neg < 0
    assert math.isnan(value)
    assert math.isnan(plus_nan)
    assert math.isnan(minus_nan)
    assert math.isinf(_expect_toml_float(array[0])) and _expect_toml_float(array[0]) > 0
    assert math.isinf(_expect_toml_float(array[1])) and _expect_toml_float(array[1]) < 0
    assert math.isnan(_expect_toml_float(array[2]))
    assert nested == [[100.0], [0.0], [-0.01]]
    assert math.isinf(
        _expect_toml_float(
            _expect_toml_array(_expect_toml_array(special_float_deeper_arrays[0])[0])[0]
        )
    )
    assert math.isinf(
        _expect_toml_float(
            _expect_toml_array(_expect_toml_array(special_float_deeper_arrays[1])[0])[0]
        )
    )
    assert math.isnan(
        _expect_toml_float(
            _expect_toml_array(_expect_toml_array(special_float_deeper_arrays[2])[0])[0]
        )
    )
    assert upper_exp_nested_mixed == [[100.0, 0.0], [-0.01]]
    assert math.isinf(_expect_toml_float(special_float_inline_table["pos"]))
    assert math.isinf(_expect_toml_float(special_float_inline_table["neg"]))
    assert math.isnan(_expect_toml_float(special_float_inline_table["nan"]))
    assert math.isinf(
        _expect_toml_float(_expect_toml_array(special_float_mixed_nested[0])[0])
    )
    assert math.isinf(
        _expect_toml_float(_expect_toml_array(special_float_mixed_nested[0])[1])
    )
    assert math.isnan(
        _expect_toml_float(_expect_toml_array(special_float_mixed_nested[1])[0])
    )
    assert len(nested_datetime_arrays) == 2
    assert str(data["positive_offset_scalar_space"]) == "1979-05-27 07:32:00+07:00"
    assert len(positive_offset_array_space) == 2
    assert len(utc_fraction_array) == 2
    assert len(utc_fraction_lower_array) == 2
    assert len(utc_fraction_lower_array_trailing) == 2


def test_toml_core_surface_is_exercised() -> None:
    name = "Alice"
    tokens = tokenize_template(t"name = {name}")
    diagnostic = Diagnostic(code="toml", message="message")

    assert {
        "tokens": [type(token).__name__ for token in tokens],
        "diagnostic": (diagnostic.code, diagnostic.severity),
        "span_type": type(SourceSpan.point(0, 0)).__name__,
        "slots_module": _slots.SlotContext.KEY.value,
    } == snapshot(
        {
            "tokens": ["StaticTextToken", "InterpolationToken"],
            "diagnostic": ("toml", DiagnosticSeverity.ERROR),
            "span_type": "SourceSpan",
            "slots_module": "key",
        }
    )


def test_toml_parse_and_value_errors_are_meaningful() -> None:
    bad_key = 3
    bad_time = time(1, 2, 3, tzinfo=UTC)
    bad_fragment = BadStringValue()

    with pytest.raises(TemplateParseError, match="Expected a TOML value"):
        render_text(t"name = ")

    with pytest.raises(
        TemplateParseError, match="single-line basic strings cannot contain newlines"
    ):
        render_text(t'value = "a\nb"')

    with pytest.raises(UnrepresentableValueError, match="TOML key"):
        render_text(t"{bad_key} = 1")

    with pytest.raises(UnrepresentableValueError, match="no null value"):
        render_text(t"name = {None}")

    with pytest.raises(UnrepresentableValueError, match="timezone"):
        render_text(t"when = {bad_time}")

    with pytest.raises(UnrepresentableValueError, match="string fragment"):
        render_text(t'title = "hi-{bad_fragment}"')

    with pytest.raises(TemplateSemanticError, match="Duplicate TOML"):
        render_data(Template('[a]\nvalue = 1\n[a]\nname = "x"\n'))

    with pytest.raises(TemplateSemanticError, match="Conflicting TOML"):
        render_data(Template('[[a]]\nname = "x"\n[a]\nvalue = 1\n'))

    with pytest.raises(TemplateParseError, match="Expected a TOML key segment"):
        render_data(Template("a.\nb = 1\n"))

    with pytest.raises(TemplateParseError, match="Expected a TOML value"):
        render_data(Template("value = [1,,2]\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = 1__2\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = 0x_DEAD\n"))

    with pytest.raises(TemplateParseError, match="Expected a TOML key segment"):
        render_data(Template("[a.]\nvalue = 1\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = 0o_7\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = 1_.0\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = 00\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = +01\n"))

    with pytest.raises(TemplateParseError, match="Expected a TOML key segment"):
        render_data(Template("value = { a = 1,, b = 2 }\n"))

    with pytest.raises(TemplateParseError, match="Expected a TOML value"):
        render_data(Template("value = [,1]\n"))

    with pytest.raises(
        TemplateParseError,
        match="Trailing commas are not permitted in TOML 1.0 inline tables",
    ):
        render_data(Template("value = { a = 1, }\n"), profile="1.0")

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = 01.2\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = 0b_1\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = 0x1.2\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = -0o7\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = { a = 1 b = 2 }\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = +inf_\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = +nan_\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = 03:04:05z\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = +0b1\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = 03:04:05+09:00\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = --1.0\n"))

    with pytest.raises(TemplateParseError, match="Expected a TOML key segment"):
        render_data(Template("a..b = 1\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = 1e_1\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = 1e1_\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = 1e--1\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = 1e1__0\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = +0_.0\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = 1_2.3__4\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = 1e1__0\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = 1e_+1\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = 1.e1\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = ++1\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = { inner = { deeper = ++1 } }\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = [[++1]]\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = [1, ++1]\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = [[[++1]]]\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = [1, 2, ++1]\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = { pos = ++1 }\n"))

    with pytest.raises(TemplateParseError, match="Invalid TOML literal"):
        render_data(Template("value = { inner = { pos = ++1 } }\n"))


def test_render_temporal_values_round_trip() -> None:
    day = date(2024, 1, 2)
    moment = time(4, 5, 6)

    assert render_data(t"day = {day}\nmoment = {moment}") == snapshot(
        {"day": day, "moment": moment}
    )


def test_toml_requires_a_template_object() -> None:
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


def test_toml_single_runtime_path_is_not_loose_mode() -> None:
    value = "Alice"

    assert {
        "text": render_text(t"name = {value}"),
        "data": render_data(t"name = {value}"),
    } == snapshot(
        {
            "text": 'name = "Alice"',
            "data": {"name": "Alice"},
        }
    )

    with pytest.raises(TemplateParseError, match="Expected a TOML value"):
        render_text(t"name = ")


def test_toml_render_result_matches_render_data_and_render_text() -> None:
    value = "Alice"
    template = t"name = {value}"

    result = render_result(template)
    assert isinstance(result, RenderResult)
    assert result.data == render_data(template)
    assert result.text == render_text(template)


def test_toml_public_exports_are_standardized() -> None:
    assert {
        "all": toml_tstring.__all__,
        "parse_identity": toml_tstring.TemplateParseError is TemplateParseError,
        "semantic_identity": toml_tstring.TemplateSemanticError
        is TemplateSemanticError,
        "unrepr_identity": toml_tstring.UnrepresentableValueError
        is UnrepresentableValueError,
        "render_result_type_identity": toml_tstring.RenderResult is CoreRenderResult,
        "imported_render_result_type_identity": RenderResult is CoreRenderResult,
        "template_error_identity": toml_tstring.TemplateError is TemplateError,
        "render_data_identity": toml_tstring.render_data is render_data,
        "render_result_identity": toml_tstring.render_result is render_result,
        "render_text_identity": toml_tstring.render_text is render_text,
    } == snapshot(
        {
            "all": [
                "RenderResult",
                "TemplateError",
                "TemplateParseError",
                "TemplateSemanticError",
                "TomlProfile",
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
