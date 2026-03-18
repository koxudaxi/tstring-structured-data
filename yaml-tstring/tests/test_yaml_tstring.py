from __future__ import annotations

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
    UnrepresentableValueError,
    YamlKey,
    YamlValue,
    tokenize_template,
)
from tstring_core import (
    RenderResult as CoreRenderResult,
)

import yaml_tstring
from yaml_tstring import RenderResult, _slots, render_data, render_result, render_text

type YamlMapping = dict[YamlKey, YamlValue]


def _is_yaml_mapping(value: YamlValue) -> TypeIs[YamlMapping]:
    return isinstance(value, dict)


def _expect_yaml_mapping(value: YamlValue) -> YamlMapping:
    assert _is_yaml_mapping(value)
    return value


def _expect_yaml_string(value: YamlValue) -> str:
    assert isinstance(value, str)
    return value


def _render_yaml_value_field(template: Template) -> YamlValue:
    return _expect_yaml_mapping(render_data(template))["value"]


class BadStringValue:
    def __str__(self) -> str:
        raise ValueError("cannot stringify")


class NestedYamlKey:
    def __str__(self) -> str:
        inner = "owner"
        return _expect_yaml_string(_render_yaml_value_field(t"value: {inner}\n"))


def test_yaml_cache_reuses_static_structure_without_stale_expression_labels() -> None:
    bad_tag_one = "bad tag"
    bad_tag_two = "other tag"

    with pytest.raises(UnrepresentableValueError, match="bad_tag_one"):
        render_text(t"!{bad_tag_one} value")

    with pytest.raises(UnrepresentableValueError, match="bad_tag_two"):
        render_text(t"!{bad_tag_two} value")


def test_yaml_end_to_end_supported_positions() -> None:
    user = "Alice"
    key = "owner"
    anchor = "item"
    tag = "str"

    template = t"""
    {key}: {user}
    label: "prefix-{user}"
    plain: item-{user}
    items:
      - &{anchor} {user}
      - *{anchor}
    tagged: !{tag} {user}
    flow: [{user}, {{label: {user}}}]
    """

    assert {
        "data": render_data(template),
        "text": render_text(template),
    } == snapshot(
        {
            "data": {
                "owner": "Alice",
                "label": "prefix-Alice",
                "plain": "item-Alice",
                "items": ["Alice", "Alice"],
                "tagged": "Alice",
                "flow": ["Alice", {"label": "Alice"}],
            },
            "text": (
                '"owner": "Alice"\nlabel: "prefix-Alice"\nplain: item-Alice\n'
                'items:\n  - &item "Alice"\n  - *item\n'
                'tagged: !str "Alice"\nflow: [ "Alice", { label: "Alice" } ]'
            ),
        }
    )


def test_yaml_block_scalar_and_sequence_item_support() -> None:
    user = "Alice"

    template = t"""
    literal: |
      hello {user}
      world
    folded: >
      hello {user}
      world
    lines:
      - |
          item {user}
    """

    assert {
        "data": render_data(template),
        "text": render_text(template),
    } == snapshot(
        {
            "data": {
                "literal": "hello Alice\nworld\n",
                "folded": "hello Alice world\n",
                "lines": ["item Alice\n"],
            },
            "text": (
                "literal: |\n  hello Alice\n  world\nfolded: >\n"
                "  hello Alice\n  world\nlines:\n  - |\n    item Alice\n"
            ),
        }
    )


def test_yaml_quoted_scalars_follow_yaml_1_2_2_escaping() -> None:
    assert {
        "double": render_data(t'value: "line\\nnext \\u03B1 \\x41"'),
        "single": render_data(t"value: 'it''s ok'"),
        "single_folded": render_data(Template("value: 'a\n  b'\n")),
        "single_leading_folded": render_data(Template("value: '\n  a'\n")),
        "empty_single": render_data(Template("value: ''\n")),
        "empty_double": render_data(t'value: ""'),
        "single_blank_lines": render_data(Template("value: 'a\n\n  b\n\n  c'\n")),
        "single_more_blank_lines": render_data(Template("value: 'a\n\n\n  b'\n")),
        "multiline_double": render_data(t'value: "a\n  b"'),
        "multiline_double_blank": render_data(t'value: "a\n\n  b"'),
        "multiline_double_more_blank": render_data(t'value: "a\n\n\n  b"'),
        "unicode_upper": render_data(t'value: "\\U0001D11E"'),
        "crlf_join": render_data(Template('value: "a\\\r\n  b"\n')),
        "special_spaces": {
            "nel": repr(_render_yaml_value_field(t'value: "\\N"')),
            "nbsp": repr(_render_yaml_value_field(t'value: "\\_"')),
        },
        "other_named_escapes": {
            "space": _render_yaml_value_field(t'value: "\\ "'),
            "slash": _render_yaml_value_field(t'value: "\\/"'),
            "tab": _render_yaml_value_field(t'value: "\\t"'),
        },
    } == snapshot(
        {
            "double": {"value": "line\nnext α A"},
            "single": {"value": "it's ok"},
            "single_folded": {"value": "a b"},
            "single_leading_folded": {"value": " a"},
            "empty_single": {"value": ""},
            "empty_double": {"value": ""},
            "single_blank_lines": {"value": "a\nb\nc"},
            "single_more_blank_lines": {"value": "a\n\nb"},
            "multiline_double": {"value": "a b"},
            "multiline_double_blank": {"value": "a\nb"},
            "multiline_double_more_blank": {"value": "a\n\nb"},
            "unicode_upper": {"value": "𝄞"},
            "crlf_join": {"value": "ab"},
            "special_spaces": {"nel": "'\\x85'", "nbsp": "'\\xa0'"},
            "other_named_escapes": {"space": " ", "slash": "/", "tab": "\t"},
        }
    )


def test_yaml_spec_quoted_scalar_examples_round_trip() -> None:
    assert {
        "unicode": render_data(t'unicode: "Sosa did fine.\\u263A"'),
        "control": render_data(t'control: "\\b1998\\t1999\\t2000\\n"'),
        "single": render_data(t"""single: '"Howdy!" he cried.'"""),
        "quoted": render_data(t"""quoted: ' # Not a ''comment''.'"""),
        "tie": render_data(t"""tie: '|\\-*-/|'"""),
    } == snapshot(
        {
            "unicode": {"unicode": "Sosa did fine.\u263a"},
            "control": {"control": "\b1998\t1999\t2000\n"},
            "single": {"single": '"Howdy!" he cried.'},
            "quoted": {"quoted": " # Not a 'comment'."},
            "tie": {"tie": "|\\-*-/|"},
        }
    )


def test_yaml_block_scalar_chomping_indicators_follow_yaml_1_2_2() -> None:
    assert {
        "literal_strip": render_data(t"value: |-\n  a\n  b\n"),
        "literal_keep": render_data(t"value: |+\n  a\n  b\n"),
        "literal_keep_leading_blank": render_data(t"value: |+\n\n  a\n"),
        "folded_strip": render_data(t"value: >-\n  a\n  b\n"),
        "folded_keep": render_data(t"value: >+\n  a\n  b\n"),
        "folded_more_indented": render_data(t"value: >\n  a\n    b\n  c\n"),
        "indent_indicator": render_data(t"value: |2\n  a\n  b\n"),
        "literal_blank_keep": render_data(t"value: |+\n  a\n\n  b\n"),
        "folded_blank_keep": render_data(t"value: >+\n  a\n\n  b\n"),
    } == snapshot(
        {
            "literal_strip": {"value": "a\nb"},
            "literal_keep": {"value": "a\nb\n"},
            "literal_keep_leading_blank": {"value": "\na\n"},
            "folded_strip": {"value": "a b"},
            "folded_keep": {"value": "a b\n"},
            "folded_more_indented": {"value": "a\n  b\nc\n"},
            "indent_indicator": {"value": "a\nb\n"},
            "literal_blank_keep": {"value": "a\n\nb\n"},
            "folded_blank_keep": {"value": "a\nb\n"},
        }
    )


def test_yaml_12_scalar_semantics_and_top_level_sequences() -> None:
    user = "Alice"

    assert {
        "mapping": render_data(
            t"""
            on: on
            yes: yes
            truth: true
            empty: null
            """
        ),
        "sequence": render_data(
            t"""
            - {user}
            - true
            - on
            """
        ),
    } == snapshot(
        {
            "mapping": {"on": "on", "yes": "yes", "truth": True, "empty": None},
            "sequence": ["Alice", True, "on"],
        }
    )


def test_yaml_multiline_plain_scalars_follow_yaml_1_2_2_folding() -> None:
    assert {
        "mapping": render_data(
            t"""
            value: a
              b
              c
            """
        ),
        "blank_line": render_data(
            t"""
            value: a

              b
            """
        ),
        "sequence": render_data(
            t"""
            - a
              b
            """
        ),
        "rendered_blank_line": render_text(
            t"""
            value: a

              b
            """
        ),
        "hash_without_space": render_data(
            t"value: a#b\n",
        ),
    } == snapshot(
        {
            "mapping": {"value": "a b c"},
            "blank_line": {"value": "a\nb"},
            "sequence": ["a b"],
            "rendered_blank_line": 'value: "a\\nb"',
            "hash_without_space": {"value": "a#b"},
        }
    )


def test_yaml_directives_streams_merge_keys_and_complex_keys() -> None:
    assert {
        "directive": render_data(
            t"%YAML 1.2\n---\nname: Alice\n...\n",
        ),
        "comment_only_document": render_data(
            t"# comment\n",
        ),
        "comment_only_explicit_document": render_data(
            t"--- # comment\n",
        ),
        "comment_only_explicit_end_document": render_data(
            t"--- # comment\n...\n",
        ),
        "comment_only_explicit_end_stream": render_data(
            t"--- # comment\n...\n---\na: 1\n",
        ),
        "comment_only_mid_stream": render_data(
            t"---\na: 1\n--- # comment\n...\n---\nb: 2\n",
        ),
        "comment_only_tail_stream": render_data(
            t"---\na: 1\n--- # comment\n...\n",
        ),
        "doc_start_comment": render_data(
            t"--- # comment\nvalue: 1\n",
        ),
        "doc_start_tag_comment": render_data(
            t"--- !!str true # comment\n",
        ),
        "tag_directive_scalar": render_data(
            t"%TAG !e! tag:example.com,2020:\n---\nvalue: !e!foo 1\n",
        ),
        "tag_directive_root": render_data(
            t"%YAML 1.2\n%TAG !e! tag:example.com,2020:\n---\n"
            t"!e!root {{value: !e!leaf 1}}\n",
        ),
        "tag_directive_root_comment": render_data(
            t"%YAML 1.2\n%TAG !e! tag:example.com,2020:\n--- # comment\n"
            t"!e!root {{value: !e!leaf 1}}\n",
        ),
        "tagged_block_root_mapping": render_data(
            t"--- !!map\na: 1\n",
        ),
        "tagged_block_root_sequence": render_data(
            t"--- !!seq\n- 1\n- 2\n",
        ),
        "verbatim_root_mapping": render_data(
            t"--- !<tag:yaml.org,2002:map>\na: 1\n",
        ),
        "verbatim_root_sequence": render_data(
            t"--- !<tag:yaml.org,2002:seq>\n- 1\n- 2\n",
        ),
        "root_anchor_mapping": render_data(
            t"--- &root\n  a: 1\n",
        ),
        "root_anchor_sequence": render_data(
            t"--- &root\n  - 1\n  - 2\n",
        ),
        "root_anchor_custom_mapping": render_data(
            t"--- &root !custom\n  a: 1\n",
        ),
        "root_custom_anchor_sequence": render_data(
            t"--- !custom &root\n  - 1\n  - 2\n",
        ),
        "stream": render_data(
            t"---\nname: Alice\n---\nname: Bob\n",
        ),
        "merge": render_data(
            t"base: &base\n  a: 1\n  b: 2\nderived:\n  <<: *base\n  c: 3\n",
        ),
        "flow_alias_mapping": render_data(
            Template("value: {left: &a 1, right: *a}\n"),
        ),
        "flow_alias_sequence": render_data(
            t"value: [&a 1, *a]\n",
        ),
        "flow_merge": render_data(
            Template("value: {<<: &base {a: 1}, b: 2}\n"),
        ),
        "flow_nested_alias_merge": render_data(
            Template("value: [{<<: &base {a: 1}, b: 2}, *base]\n"),
        ),
        "empty_flow_sequence": render_data(Template("value: []\n")),
        "empty_flow_mapping": render_data(Template("value: {}\n")),
        "flow_scalar_mix": render_data(Template("value: [\"\", '', plain]\n")),
        "flow_plain_scalar_with_space": render_data(Template("value: [1 2]\n")),
        "mapping_empty_flow_values": render_data(Template("value: {a: [], b: {}}\n")),
        "flow_mapping_empty_key_and_values": render_data(
            Template('{"": [], foo: {}}\n')
        ),
        "flow_mapping_nested_empty": render_data(Template("{a: {}, b: []}\n")),
        "flow_null_key": render_data(Template('{null: 1, "": 2}\n')),
        "block_null_key": render_data(Template("? null\n: 1\n")),
        "quoted_null_key": render_data(Template('? ""\n: 1\n')),
        "plain_question_mark_scalar": render_data(Template("value: ?x\n")),
        "plain_colon_scalar_flow": render_data(Template("value: [a:b, c:d]\n")),
        "flow_mapping_plain_key_question": render_data(Template("value: {?x: 1}\n")),
        "flow_mapping_plain_key_questions": render_data(
            Template("value: {?x: 1, ?y: 2}\n")
        ),
        "flow_hash_plain_mapping_value": render_data(Template("value: {a: b#c}\n")),
        "flow_hash_plain_mapping_values": render_data(
            Template("value: {a: b#c, d: e#f}\n")
        ),
        "flow_hash_plain_scalars": render_data(Template("value: [a#b, c#d]\n")),
        "flow_hash_value_sequence": render_data(Template("value: [a#b, c#d, e#f]\n")),
        "flow_hash_long_sequence": render_data(
            Template("value: [a#b, c#d, e#f, g#h]\n")
        ),
        "flow_hash_five_sequence": render_data(
            Template("value: [a#b, c#d, e#f, g#h, i#j]\n")
        ),
        "flow_mapping_hash_key": render_data(Template("value: {a#b: 1}\n")),
        "flow_sequence_comments_value": render_data(Template("value: [1, # c\n 2]\n")),
        "flow_mapping_comments_value": render_data(
            Template("value: {a: 1, # c\n b: 2}\n")
        ),
        "comment_after_value": render_data(Template("value: a # c\n")),
        "plain_colon_hash": render_data(Template("value: a:b#c\n")),
        "plain_colon_hash_deeper": render_data(Template("value: a:b:c#d\n")),
        "plain_hash_chain": render_data(Template("value: a#b#c\n")),
        "plain_hash_chain_deeper": render_data(Template("value: a#b#c#d\n")),
        "plain_hash_chain_deeper_comment": render_data(
            Template("value: a#b#c#d # comment\n")
        ),
        "flow_hash_mapping_long": render_data(
            Template("value: {a: b#c, d: e#f, g: h#i}\n")
        ),
        "flow_hash_mapping_four": render_data(
            Template("value: {a: b#c, d: e#f, g: h#i, j: k#l}\n")
        ),
        "flow_hash_mapping_five": render_data(
            Template("value: {a: b#c, d: e#f, g: h#i, j: k#l, m: n#o}\n")
        ),
        "flow_hash_mapping_six": render_data(
            Template("value: {a: b#c, d: e#f, g: h#i, j: k#l, m: n#o, p: q#r}\n")
        ),
        "comment_after_plain_colon": render_data(Template("value: a:b # c\n")),
        "comment_after_flow_plain_colon": render_data(Template("value: [a:b # c\n]\n")),
        "flow_plain_hash_chain": render_data(Template("value: [a#b#c, d#e#f]\n")),
        "flow_plain_hash_chain_single_deeper": render_data(
            Template("value: [a#b#c#d]\n")
        ),
        "flow_plain_hash_chain_single_deeper_comment": render_data(
            Template("value: [a#b#c#d # comment\n]\n")
        ),
        "flow_hash_seq_six": render_data(
            Template("value: [a#b, c#d, e#f, g#h, i#j, k#l]\n")
        ),
        "flow_hash_seq_seven": render_data(
            Template("value: [a#b, c#d, e#f, g#h, i#j, k#l, m#n]\n")
        ),
        "flow_plain_hash_chain_long": render_data(
            Template("value: [a#b#c, d#e#f, g#h#i]\n")
        ),
        "flow_plain_hash_chain_four": render_data(
            Template("value: [a#b#c, d#e#f, g#h#i, j#k#l]\n")
        ),
        "block_plain_comment_after_colon_long": render_data(
            Template("value: a:b:c # comment\n")
        ),
        "block_plain_comment_after_colon_deeper": render_data(
            Template("value: a:b:c:d # comment\n")
        ),
        "flow_plain_comment_after_colon_long": render_data(
            Template("value: [a:b:c # comment\n]\n")
        ),
        "flow_plain_comment_after_colon_deeper": render_data(
            Template("value: [a:b:c:d # comment\n]\n")
        ),
        "flow_plain_colon_hash_deeper": render_data(Template("value: [a:b:c#d]\n")),
        "flow_mapping_plain_key_with_colon": render_data(Template("value: {a:b: c}\n")),
        "flow_mapping_colon_and_hash": render_data(Template("value: {a:b: c#d}\n")),
        "block_plain_colon_no_space": render_data(Template("value: a:b:c\n")),
        "alias_in_flow_mapping_value": render_data(
            Template("base: &a {x: 1}\nvalue: {ref: *a}\n")
        ),
        "flow_null_and_alias": render_data(
            Template("base: &a {x: 1}\nvalue: {null: *a}\n")
        ),
        "flow_mapping_missing_value": render_data(Template("value: {a: }\n")),
        "flow_seq_missing_value_before_end": render_data(Template("value: [1, 2, ]\n")),
        "complex_key": render_data(
            t"? [a, b]\n: 1\n",
        ),
        "verbatim_tag": render_data(
            t"value: !<tag:yaml.org,2002:str> hello\n",
        ),
        "custom_tag_scalar": render_data(
            t"value: !custom 3\n",
        ),
        "custom_tag_sequence": render_data(
            t"value: !custom [1, 2]\n",
        ),
        "flow_wrapped_sequence": render_data(
            t"key: [a,\n  b]\n",
        ),
        "flow_wrapped_mapping": render_data(
            Template("key: {a: 1,\n  b: 2}\n"),
        ),
        "flow_sequence_comment": render_data(
            t"key: [a, # first\n  b]\n",
        ),
        "flow_mapping_comment": render_data(
            Template("key: {a: 1, # first\n  b: 2}\n"),
        ),
        "alias_seq_value": render_data(
            Template("a: &x [1, 2]\nb: *x\n"),
        ),
        "empty_document": render_data(
            t"---\n...\n",
        ),
        "empty_document_stream": render_data(
            t"---\n\n---\na: 1\n",
        ),
        "indentless_sequence_value": render_data(
            t"a:\n- 1\n- 2\n",
        ),
        "empty_explicit_key": render_data(
            t"?\n: 1\n",
        ),
        "sequence_of_mappings": render_data(
            t"- a: 1\n  b: 2\n- c: 3\n",
        ),
        "mapping_of_sequence_of_mappings": render_data(
            t"items:\n- a: 1\n  b: 2\n- c: 3\n",
        ),
        "sequence_of_sequences": render_data(
            t"- - 1\n  - 2\n- - 3\n",
        ),
        "flow_newline": render_text(Template("{a: 1, b: [2, 3]}\n")),
        "explicit_end_stream": render_text(Template("---\na: 1\n...\n---\nb: 2\n")),
        "explicit_end_comment_stream": render_data(
            t"---\na: 1\n... # end\n---\nb: 2\n",
        ),
        "tag_directive_text": render_text(
            t"%TAG !e! tag:example.com,2020:\n---\nvalue: !e!foo 1\n",
        ),
        "tag_directive_root_comment_text": render_text(
            t"%YAML 1.2\n%TAG !e! tag:example.com,2020:\n--- # comment\n"
            t"!e!root {{value: !e!leaf 1}}\n",
        ),
        "root_anchor_mapping_text": render_text(
            t"--- &root\n  a: 1\n",
        ),
        "root_anchor_sequence_text": render_text(
            t"--- &root\n  - 1\n  - 2\n",
        ),
        "verbatim_root_mapping_text": render_text(
            t"--- !<tag:yaml.org,2002:map>\na: 1\n",
        ),
        "verbatim_root_sequence_text": render_text(
            t"--- !<tag:yaml.org,2002:seq>\n- 1\n- 2\n",
        ),
    } == snapshot(
        {
            "directive": {"name": "Alice"},
            "comment_only_document": None,
            "comment_only_explicit_document": None,
            "comment_only_explicit_end_document": None,
            "comment_only_explicit_end_stream": [None, {"a": 1}],
            "comment_only_mid_stream": [{"a": 1}, None, {"b": 2}],
            "comment_only_tail_stream": [{"a": 1}, None],
            "doc_start_comment": {"value": 1},
            "doc_start_tag_comment": "true",
            "tag_directive_scalar": {"value": 1},
            "tag_directive_root": {"value": 1},
            "tag_directive_root_comment": {"value": 1},
            "tagged_block_root_mapping": {"a": 1},
            "tagged_block_root_sequence": [1, 2],
            "verbatim_root_mapping": {"a": 1},
            "verbatim_root_sequence": [1, 2],
            "root_anchor_mapping": {"a": 1},
            "root_anchor_sequence": [1, 2],
            "root_anchor_custom_mapping": {"a": 1},
            "root_custom_anchor_sequence": [1, 2],
            "stream": [{"name": "Alice"}, {"name": "Bob"}],
            "merge": {
                "base": {"a": 1, "b": 2},
                "derived": {"a": 1, "b": 2, "c": 3},
            },
            "flow_alias_mapping": {"value": {"left": 1, "right": 1}},
            "flow_alias_sequence": {"value": [1, 1]},
            "flow_merge": {"value": {"a": 1, "b": 2}},
            "flow_nested_alias_merge": {"value": [{"a": 1, "b": 2}, {"a": 1}]},
            "empty_flow_sequence": {"value": []},
            "empty_flow_mapping": {"value": {}},
            "flow_scalar_mix": {"value": ["", "", "plain"]},
            "flow_plain_scalar_with_space": {"value": ["1 2"]},
            "mapping_empty_flow_values": {"value": {"a": [], "b": {}}},
            "flow_mapping_empty_key_and_values": {"": [], "foo": {}},
            "flow_mapping_nested_empty": {"a": {}, "b": []},
            "flow_null_key": {None: 1, "": 2},
            "block_null_key": {None: 1},
            "quoted_null_key": {"": 1},
            "plain_question_mark_scalar": {"value": "?x"},
            "plain_colon_scalar_flow": {"value": ["a:b", "c:d"]},
            "flow_mapping_plain_key_question": {"value": {"x": 1}},
            "flow_mapping_plain_key_questions": {"value": {"x": 1, "y": 2}},
            "flow_hash_plain_mapping_value": {"value": {"a": "b#c"}},
            "flow_hash_plain_mapping_values": {"value": {"a": "b#c", "d": "e#f"}},
            "flow_hash_plain_scalars": {"value": ["a#b", "c#d"]},
            "flow_hash_value_sequence": {"value": ["a#b", "c#d", "e#f"]},
            "flow_hash_long_sequence": {"value": ["a#b", "c#d", "e#f", "g#h"]},
            "flow_hash_five_sequence": {"value": ["a#b", "c#d", "e#f", "g#h", "i#j"]},
            "flow_mapping_hash_key": {"value": {"a#b": 1}},
            "flow_sequence_comments_value": {"value": [1, 2]},
            "flow_mapping_comments_value": {"value": {"a": 1, "b": 2}},
            "comment_after_value": {"value": "a"},
            "plain_colon_hash": {"value": "a:b#c"},
            "plain_colon_hash_deeper": {"value": "a:b:c#d"},
            "plain_hash_chain": {"value": "a#b#c"},
            "plain_hash_chain_deeper": {"value": "a#b#c#d"},
            "plain_hash_chain_deeper_comment": {"value": "a#b#c#d"},
            "flow_hash_mapping_long": {"value": {"a": "b#c", "d": "e#f", "g": "h#i"}},
            "flow_hash_mapping_four": {
                "value": {"a": "b#c", "d": "e#f", "g": "h#i", "j": "k#l"}
            },
            "flow_hash_mapping_five": {
                "value": {"a": "b#c", "d": "e#f", "g": "h#i", "j": "k#l", "m": "n#o"}
            },
            "flow_hash_mapping_six": {
                "value": {
                    "a": "b#c",
                    "d": "e#f",
                    "g": "h#i",
                    "j": "k#l",
                    "m": "n#o",
                    "p": "q#r",
                }
            },
            "comment_after_plain_colon": {"value": "a:b"},
            "comment_after_flow_plain_colon": {"value": ["a:b"]},
            "flow_plain_hash_chain": {"value": ["a#b#c", "d#e#f"]},
            "flow_plain_hash_chain_single_deeper": {"value": ["a#b#c#d"]},
            "flow_plain_hash_chain_single_deeper_comment": {"value": ["a#b#c#d"]},
            "flow_hash_seq_six": {"value": ["a#b", "c#d", "e#f", "g#h", "i#j", "k#l"]},
            "flow_hash_seq_seven": {
                "value": ["a#b", "c#d", "e#f", "g#h", "i#j", "k#l", "m#n"]
            },
            "flow_plain_hash_chain_long": {"value": ["a#b#c", "d#e#f", "g#h#i"]},
            "flow_plain_hash_chain_four": {
                "value": ["a#b#c", "d#e#f", "g#h#i", "j#k#l"]
            },
            "block_plain_comment_after_colon_long": {"value": "a:b:c"},
            "block_plain_comment_after_colon_deeper": {"value": "a:b:c:d"},
            "flow_plain_comment_after_colon_long": {"value": ["a:b:c"]},
            "flow_plain_comment_after_colon_deeper": {"value": ["a:b:c:d"]},
            "flow_plain_colon_hash_deeper": {"value": ["a:b:c#d"]},
            "flow_mapping_plain_key_with_colon": {"value": {"a:b": "c"}},
            "flow_mapping_colon_and_hash": {"value": {"a:b": "c#d"}},
            "block_plain_colon_no_space": {"value": "a:b:c"},
            "alias_in_flow_mapping_value": {
                "base": {"x": 1},
                "value": {"ref": {"x": 1}},
            },
            "flow_null_and_alias": {
                "base": {"x": 1},
                "value": {None: {"x": 1}},
            },
            "flow_mapping_missing_value": {"value": {"a": None}},
            "flow_seq_missing_value_before_end": {"value": [1, 2]},
            "complex_key": {("a", "b"): 1},
            "verbatim_tag": {"value": "hello"},
            "custom_tag_scalar": {"value": 3},
            "custom_tag_sequence": {"value": [1, 2]},
            "flow_wrapped_sequence": {"key": ["a", "b"]},
            "flow_wrapped_mapping": {"key": {"a": 1, "b": 2}},
            "flow_sequence_comment": {"key": ["a", "b"]},
            "flow_mapping_comment": {"key": {"a": 1, "b": 2}},
            "alias_seq_value": {"a": [1, 2], "b": [1, 2]},
            "empty_document": None,
            "empty_document_stream": [None, {"a": 1}],
            "indentless_sequence_value": {"a": [1, 2]},
            "empty_explicit_key": {None: 1},
            "sequence_of_mappings": [{"a": 1, "b": 2}, {"c": 3}],
            "mapping_of_sequence_of_mappings": {"items": [{"a": 1, "b": 2}, {"c": 3}]},
            "sequence_of_sequences": [[1, 2], [3]],
            "flow_newline": "{ a: 1, b: [ 2, 3 ] }",
            "explicit_end_stream": "---\na: 1\n...\n---\nb: 2",
            "explicit_end_comment_stream": [{"a": 1}, {"b": 2}],
            "tag_directive_text": (
                "%TAG !e! tag:example.com,2020:\n---\nvalue: !e!foo 1"
            ),
            "tag_directive_root_comment_text": (
                "%YAML 1.2\n%TAG !e! tag:example.com,2020:\n---\n"
                "!e!root { value: !e!leaf 1 }"
            ),
            "root_anchor_mapping_text": "---\n&root\na: 1",
            "root_anchor_sequence_text": "---\n&root\n- 1\n- 2",
            "verbatim_root_mapping_text": "---\n!<tag:yaml.org,2002:map>\na: 1",
            "verbatim_root_sequence_text": "---\n!<tag:yaml.org,2002:seq>\n- 1\n- 2",
        }
    )


def test_yaml_nested_render_in_key_coercion_is_safe() -> None:
    key = NestedYamlKey()

    assert render_data(t"{key}: 1\n") == {"owner": 1}


def test_yaml_preserves_exact_large_python_integers() -> None:
    value = 2**100
    key = 2**100

    assert render_data(t"value: {value}\n") == {"value": value}
    assert render_text(t"value: {value}\n") == f"value: {value}"
    assert render_data(t"{key}: ok\n") == {key: "ok"}
    assert render_text(t"{key}: ok\n") == f"{key}: ok"


def test_yaml_collection_interpolation_layout_switches_without_changing_data() -> None:
    mapping = {
        "yes": "on",
        "0123": "a # b",
        "nested": {
            "empty_list": [],
            "empty_map": {},
            "a: b": "yes",
        },
    }
    items = [1, 2]
    empty_mapping: dict[str, object] = {}
    empty_list: list[object] = []
    tag = "custom"
    anchor = "root"

    block_mapping = t"value: {mapping}\n"
    root_mapping = t"{mapping}\n"
    root_sequence = t"{items}\n"
    decorated_tag = t"value: !{tag} {mapping}\n"
    decorated_anchor = t"value: &{anchor} {items}\n"
    decorated_both = t"value: !{tag} &{anchor} {mapping}\n"
    flow_sequence = t"flow: [{mapping}]\n"
    flow_mapping = t"flow: {{k: {items}}}\n"
    empties = t"value_map: {empty_mapping}\nvalue_list: {empty_list}\n"

    assert {
        "block_mapping_text": render_text(block_mapping),
        "root_mapping_text": render_text(root_mapping),
        "root_sequence_text": render_text(root_sequence),
        "decorated_tag_text": render_text(decorated_tag),
        "decorated_anchor_text": render_text(decorated_anchor),
        "decorated_both_text": render_text(decorated_both),
        "flow_sequence_text": render_text(flow_sequence),
        "flow_mapping_text": render_text(flow_mapping),
        "empty_text": render_text(empties),
        "block_mapping_data": render_data(block_mapping),
        "root_mapping_data": render_data(root_mapping),
        "root_sequence_data": render_data(root_sequence),
        "decorated_both_data": render_data(decorated_both),
        "render_result_text": render_result(decorated_both).text,
    } == snapshot(
        {
            "block_mapping_text": (
                'value:\n  "yes": "on"\n  "0123": "a # b"\n  "nested":\n'
                '    "empty_list": []\n    "empty_map": {}\n    "a: b": "yes"'
            ),
            "root_mapping_text": (
                '"yes": "on"\n"0123": "a # b"\n"nested":\n  "empty_list": []\n'
                '  "empty_map": {}\n  "a: b": "yes"'
            ),
            "root_sequence_text": "- 1\n- 2",
            "decorated_tag_text": (
                'value: !custom\n  "yes": "on"\n  "0123": "a # b"\n  "nested":\n'
                '    "empty_list": []\n    "empty_map": {}\n    "a: b": "yes"'
            ),
            "decorated_anchor_text": "value: &root\n  - 1\n  - 2",
            "decorated_both_text": (
                'value: !custom &root\n  "yes": "on"\n  "0123": "a # b"\n'
                '  "nested":\n    "empty_list": []\n    "empty_map": {}\n'
                '    "a: b": "yes"'
            ),
            "flow_sequence_text": (
                'flow: [ { "yes": "on", "0123": "a # b", "nested": { '
                '"empty_list": [], "empty_map": {}, "a: b": "yes" } } ]'
            ),
            "flow_mapping_text": "flow: { k: [ 1, 2 ] }",
            "empty_text": "value_map: {}\nvalue_list: []",
            "block_mapping_data": {
                "value": {
                    "yes": "on",
                    "0123": "a # b",
                    "nested": {
                        "empty_list": [],
                        "empty_map": {},
                        "a: b": "yes",
                    },
                }
            },
            "root_mapping_data": {
                "yes": "on",
                "0123": "a # b",
                "nested": {
                    "empty_list": [],
                    "empty_map": {},
                    "a: b": "yes",
                },
            },
            "root_sequence_data": [1, 2],
            "decorated_both_data": {
                "value": {
                    "yes": "on",
                    "0123": "a # b",
                    "nested": {
                        "empty_list": [],
                        "empty_map": {},
                        "a: b": "yes",
                    },
                }
            },
            "render_result_text": (
                'value: !custom &root\n  "yes": "on"\n  "0123": "a # b"\n'
                '  "nested":\n    "empty_list": []\n    "empty_map": {}\n'
                '    "a: b": "yes"'
            ),
        }
    )


def test_yaml_custom_tags_validate_and_normalize_to_plain_python_data() -> None:
    assert {
        "scalar_data": render_data(t"!custom 3\n"),
        "mapping_data": render_data(t"value: !custom 3\n"),
        "sequence_data": render_data(t"value: !custom [1, 2]\n"),
        "commented_root_sequence": render_data(t"--- # comment\n!custom [1, 2]\n"),
        "scalar_text": render_text(t"!custom 3\n"),
        "mapping_text": render_text(t"value: !custom 3\n"),
    } == snapshot(
        {
            "scalar_data": 3,
            "mapping_data": {"value": 3},
            "sequence_data": {"value": [1, 2]},
            "commented_root_sequence": [1, 2],
            "scalar_text": "!custom 3",
            "mapping_text": "value: !custom 3",
        }
    )


def test_yaml_verbatim_root_scalar_tag_round_trip() -> None:
    assert {
        "data": render_data(t"--- !<tag:yaml.org,2002:str> hello\n"),
        "text": render_text(t"--- !<tag:yaml.org,2002:str> hello\n"),
    } == snapshot(
        {
            "data": "hello",
            "text": "---\n!<tag:yaml.org,2002:str> hello",
        }
    )


def test_yaml_verbatim_root_anchor_scalar_tag_round_trip() -> None:
    assert {
        "data": render_data(t"--- !<tag:yaml.org,2002:str> &root hello\n"),
        "text": render_text(t"--- !<tag:yaml.org,2002:str> &root hello\n"),
    } == snapshot(
        {
            "data": "hello",
            "text": "---\n!<tag:yaml.org,2002:str> &root hello",
        }
    )


def test_yaml_spec_chapter_2_examples_round_trip() -> None:
    assert {
        "players": render_data(
            t"""
            - Mark McGwire
            - Sammy Sosa
            - Ken Griffey
            """
        ),
        "clubs": render_data(
            t"""
            american:
            - Boston Red Sox
            - Detroit Tigers
            - New York Yankees
            national:
            - New York Mets
            - Chicago Cubs
            - Atlanta Braves
            """
        ),
        "stats_seq": render_data(
            t"""
            -
              name: Mark McGwire
              hr:   65
              avg:  0.278
            -
              name: Sammy Sosa
              hr:   63
              avg:  0.288
            """
        ),
        "map_of_maps": render_data(
            t"""
            Mark McGwire: {{hr: 65, avg: 0.278}}
            Sammy Sosa: {{
              hr: 63,
              avg: 0.288,
            }}
            """
        ),
        "two_docs": render_data(
            t"# Ranking of 1998 home runs\n---\n- Mark McGwire\n- Sammy Sosa\n"
            t"- Ken Griffey\n\n# Team ranking\n---\n- Chicago Cubs\n"
            t"- St Louis Cardinals\n"
        ),
        "play_feed": render_data(
            t"---\ntime: 20:03:20\nplayer: Sammy Sosa\naction: strike (miss)\n"
            t"...\n---\ntime: 20:03:47\nplayer: Sammy Sosa\n"
            t"action: grand slam\n...\n"
        ),
    } == snapshot(
        {
            "players": ["Mark McGwire", "Sammy Sosa", "Ken Griffey"],
            "clubs": {
                "american": [
                    "Boston Red Sox",
                    "Detroit Tigers",
                    "New York Yankees",
                ],
                "national": [
                    "New York Mets",
                    "Chicago Cubs",
                    "Atlanta Braves",
                ],
            },
            "stats_seq": [
                {"name": "Mark McGwire", "hr": 65, "avg": 0.278},
                {"name": "Sammy Sosa", "hr": 63, "avg": 0.288},
            ],
            "map_of_maps": {
                "Mark McGwire": {"hr": 65, "avg": 0.278},
                "Sammy Sosa": {"hr": 63, "avg": 0.288},
            },
            "two_docs": [
                ["Mark McGwire", "Sammy Sosa", "Ken Griffey"],
                ["Chicago Cubs", "St Louis Cardinals"],
            ],
            "play_feed": [
                {
                    "time": "20:03:20",
                    "player": "Sammy Sosa",
                    "action": "strike (miss)",
                },
                {
                    "time": "20:03:47",
                    "player": "Sammy Sosa",
                    "action": "grand slam",
                },
            ],
        }
    )

    assert {
        "players": render_text(
            t"""
            - Mark McGwire
            - Sammy Sosa
            - Ken Griffey
            """
        ),
        "clubs": render_text(
            t"""
            american:
            - Boston Red Sox
            - Detroit Tigers
            - New York Yankees
            national:
            - New York Mets
            - Chicago Cubs
            - Atlanta Braves
            """
        ),
        "stats_seq": render_text(
            t"""
            -
              name: Mark McGwire
              hr:   65
              avg:  0.278
            -
              name: Sammy Sosa
              hr:   63
              avg:  0.288
            """
        ),
        "map_of_maps": render_text(
            t"""
            Mark McGwire: {{hr: 65, avg: 0.278}}
            Sammy Sosa: {{
              hr: 63,
              avg: 0.288,
            }}
            """
        ),
        "two_docs": render_text(
            t"# Ranking of 1998 home runs\n---\n- Mark McGwire\n- Sammy Sosa\n"
            t"- Ken Griffey\n\n# Team ranking\n---\n- Chicago Cubs\n"
            t"- St Louis Cardinals\n"
        ),
        "play_feed": render_text(
            t"---\ntime: 20:03:20\nplayer: Sammy Sosa\naction: strike (miss)\n"
            t"...\n---\ntime: 20:03:47\nplayer: Sammy Sosa\n"
            t"action: grand slam\n...\n"
        ),
    } == snapshot(
        {
            "players": "- Mark McGwire\n- Sammy Sosa\n- Ken Griffey",
            "clubs": (
                "american:\n  - Boston Red Sox\n  - Detroit Tigers\n"
                "  - New York Yankees\nnational:\n  - New York Mets\n"
                "  - Chicago Cubs\n  - Atlanta Braves"
            ),
            "stats_seq": (
                "-\n  name: Mark McGwire\n  hr: 65\n  avg: 0.278\n-\n"
                "  name: Sammy Sosa\n  hr: 63\n  avg: 0.288"
            ),
            "map_of_maps": (
                "Mark McGwire: { hr: 65, avg: 0.278 }\n"
                "Sammy Sosa: { hr: 63, avg: 0.288 }"
            ),
            "two_docs": (
                "---\n- Mark McGwire\n- Sammy Sosa\n- Ken Griffey\n"
                "---\n- Chicago Cubs\n- St Louis Cardinals"
            ),
            "play_feed": (
                "---\ntime: 20:03:20\nplayer: Sammy Sosa\n"
                "action: strike (miss)\n...\n---\ntime: 20:03:47\n"
                "player: Sammy Sosa\naction: grand slam\n..."
            ),
        }
    )


def test_yaml_conformance_regressions_for_multiline_keys_and_explicit_nulls() -> None:
    assert {
        "multiline_plain": render_data(
            Template(
                "plain:\n"
                "  This unquoted scalar\n"
                "  spans many lines.\n"
                "\n"
                'quoted: "So does this\n'
                '  quoted scalar.\\n"\n'
            )
        ),
        "punctuation_keys": render_data(
            Template(
                "a!\"#$%&'()*+,-./09:;<=>?@AZ[\\]^_`az{|}~: safe\n"
                "?foo: safe question mark\n"
                ":foo: safe colon\n"
                "-foo: safe dash\n"
                "this is#not: a comment\n"
            )
        ),
        "decorated_keys": render_data(
            Template(
                "---\n"
                "top1: &node1\n"
                "  &k1 key1: one\n"
                "top2: &node2 # comment\n"
                "  key2: two\n"
                "top3:\n"
                "  &k3 key3: three\n"
                "top6: &val6\n"
                "  six\n"
                "top7:\n"
                "  &val7 seven\n"
            )
        ),
        "explicit_nulls": render_data(
            Template("--- !!set\n? Mark McGwire\n? Sammy Sosa\n? Ken Griff\n")
        ),
        "missing_values": render_data(Template("? a\n? b\nc:\n")),
        "explicit_key_value": render_data(
            Template(
                "? explicit key # Empty value\n"
                "? |\n"
                "  block key\n"
                ": - one # Explicit compact\n"
                "  - two # block value\n"
            )
        ),
    } == snapshot(
        {
            "multiline_plain": {
                "plain": "This unquoted scalar spans many lines.",
                "quoted": "So does this quoted scalar.\n",
            },
            "punctuation_keys": {
                "a!\"#$%&'()*+,-./09:;<=>?@AZ[\\]^_`az{|}~": "safe",
                "?foo": "safe question mark",
                ":foo": "safe colon",
                "-foo": "safe dash",
                "this is#not": "a comment",
            },
            "decorated_keys": {
                "top1": {"key1": "one"},
                "top2": {"key2": "two"},
                "top3": {"key3": "three"},
                "top6": "six",
                "top7": "seven",
            },
            "explicit_nulls": {"Ken Griff", "Mark McGwire", "Sammy Sosa"},
            "missing_values": {"a": None, "b": None, "c": None},
            "explicit_key_value": {"block key\n": ["one", "two"], "explicit key": None},
        }
    )


def test_yaml_explicit_core_tags_follow_yaml_1_2_core_schema() -> None:
    assert {
        "mapping": render_data(
            t"value_bool: !!bool true\nvalue_str: !!str true\n"
            t"value_float: !!float 1\nvalue_null: !!null null\n"
        ),
        "root_int": render_data(t"--- !!int 3\n"),
        "root_str": render_data(t"--- !!str true\n"),
        "root_bool": render_data(t"--- !!bool true\n"),
        "root_int_text": render_text(t"--- !!int 3\n"),
    } == snapshot(
        {
            "mapping": {
                "value_bool": True,
                "value_str": "true",
                "value_float": 1.0,
                "value_null": None,
            },
            "root_int": 3,
            "root_str": "true",
            "root_bool": True,
            "root_int_text": "---\n!!int 3",
        }
    )


def test_yaml_complex_mapping_keys_normalize_to_hashable_plain_data() -> None:
    left = "Alice"
    right = "Bob"
    normalized_key = frozenset({("name", (left, right))})

    assert render_data(
        t"""
        ? {{"name": [{left}, {right}]}}
        : 1
        """
    ) == {normalized_key: 1}

    assert render_data(
        t"{{ {{name: [{left}, {right}]}}: 1, [{left}, {right}]: 2 }}"
    ) == {
        normalized_key: 1,
        (left, right): 2,
    }

    assert render_text(
        t"{{ {{name: [{left}, {right}]}}: 1, [{left}, {right}]: 2 }}"
    ) == snapshot('{ { name: [ "Alice", "Bob" ] }: 1, [ "Alice", "Bob" ]: 2 }')


def test_yaml_edge_cases_for_flow_sequences_and_indent_indicators() -> None:
    assert {
        "flow_sequence": render_data(t"[1, 2,]\n"),
        "flow_mapping": render_data(Template("{a: 1,}\n")),
        "explicit_key_sequence_value": render_data(
            t"? a\n: - 1\n  - 2\n",
        ),
        "indent_indicator": render_data(
            t"value: |1\n a\n b\n",
        ),
        "indent_indicator_text": render_text(
            t"value: |1\n a\n b\n",
        ),
    } == snapshot(
        {
            "flow_sequence": [1, 2],
            "flow_mapping": {"a": 1},
            "explicit_key_sequence_value": {"a": [1, 2]},
            "indent_indicator": {"value": "a\nb\n"},
            "indent_indicator_text": "value: |1\n a\n b\n",
        }
    )


def test_yaml_core_surface_is_exercised() -> None:
    user = "Alice"
    tokens = tokenize_template(t"name: {user}")
    diagnostic = Diagnostic(code="yaml", message="message")

    assert {
        "tokens": [type(token).__name__ for token in tokens],
        "diagnostic": (diagnostic.code, diagnostic.severity),
        "span_type": type(SourceSpan.point(0, 0)).__name__,
        "slots_module": _slots.SlotContext.VALUE.value,
    } == snapshot(
        {
            "tokens": ["StaticTextToken", "InterpolationToken"],
            "diagnostic": ("yaml", DiagnosticSeverity.ERROR),
            "span_type": "SourceSpan",
            "slots_module": "value",
        }
    )


def test_yaml_errors_cover_parse_render_and_metadata_paths() -> None:
    bad = BadStringValue()
    tag = "bad tag"

    with pytest.raises(TemplateParseError, match="Expected"):
        render_text(t"value: [1, 2")

    with pytest.raises(TemplateParseError, match="Expected"):
        render_text(Template("{a: 1 b: 2}\n"))

    with pytest.raises(TemplateParseError, match="Expected a YAML value"):
        render_text(Template("[1,,2]\n"))

    with pytest.raises(TemplateParseError, match="Expected"):
        render_text(Template("value: [1, 2,,]\n"))

    with pytest.raises(TemplateParseError, match="Expected ':' in YAML template"):
        render_text(Template("value: {,}\n"))

    with pytest.raises(TemplateParseError, match="Expected ':' in YAML template"):
        render_text(Template("value: {a b}\n"))

    with pytest.raises(TemplateParseError, match="Tabs are not allowed"):
        render_text(Template("a:\t1\n"))

    with pytest.raises(TemplateParseError, match="Tabs are not allowed"):
        render_text(Template("url: a:b\t\n"))

    with pytest.raises(TemplateParseError, match="Tabs are not allowed"):
        render_text(Template("a:\n\t- 1\n"))

    with pytest.raises(TemplateParseError, match="Tabs are not allowed"):
        render_text(Template("a:\n  b:\n\t- 1\n"))

    with pytest.raises(TemplateParseError, match="Unexpected trailing YAML content"):
        render_text(Template("value: *not alias\n"))

    with pytest.raises(TemplateParseError, match="Expected"):
        render_text(Template("[,]\n"))

    with pytest.raises(TemplateSemanticError, match="unknown anchor"):
        render_text(Template("value: *not_alias\n"))

    with pytest.raises(TemplateSemanticError, match="unknown anchor"):
        render_text(Template("--- &a\n- 1\n- 2\n---\n*a\n"))

    with pytest.raises(TemplateParseError, match="Expected ':' in YAML template"):
        render_text(Template("value: {a: 1,, b: 2}\n"))

    with pytest.raises(TemplateParseError, match="Expected ':' in YAML template"):
        render_text(Template("value: {a: 1, , b: 2}\n"))

    with pytest.raises(
        TemplateParseError,
        match="Quote YAML plain scalars that mix whitespace and interpolations",
    ):
        render_text(t"value: fdsa fff fds{1}")

    with pytest.raises(UnrepresentableValueError, match="fragment"):
        render_text(t'label: "hi-{bad}"')

    with pytest.raises(UnrepresentableValueError, match="metadata"):
        render_text(t"value: !{tag} ok")

    with pytest.raises(UnrepresentableValueError, match="non-finite float"):
        render_text(t"value: {float('inf')}")


def test_yaml_requires_a_template_object() -> None:
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


def test_yaml_single_runtime_path_is_not_loose_mode() -> None:
    value = "Alice"
    number = 1

    assert {
        "text": render_text(t"name: {value}"),
        "data": render_data(t"name: {value}"),
        "tagged_data": render_data(t"value: !!str {number}"),
        "custom_quoted_data": render_data(t'value: !custom "01"'),
        "verbatim_quoted_data": render_data(
            t'value: !<tag:example.com,2020:custom> "01"'
        ),
    } == snapshot(
        {
            "text": 'name: "Alice"',
            "data": {"name": "Alice"},
            "tagged_data": {"value": "1"},
            "custom_quoted_data": {"value": "01"},
            "verbatim_quoted_data": {"value": "01"},
        }
    )

    with pytest.raises(TemplateParseError, match="Expected"):
        render_text(t"value: [1, 2")

    with pytest.raises(TemplateSemanticError, match="unknown anchor"):
        render_data(Template("value: *not_alias\n"))

    assert render_data(Template("first: &a 1\nsecond: &a 2\nref: *a\n")) == {
        "first": 1,
        "second": 2,
        "ref": 2,
    }


def test_yaml_render_result_matches_render_data_and_render_text() -> None:
    value = "Alice"
    template = t"name: {value}"
    single_quoted_template = Template("value: 'a\n\n  b'\n")

    result = render_result(template)
    assert isinstance(result, RenderResult)
    assert result.data == render_data(template)
    assert result.text == render_text(template)

    single_quoted_result = render_result(single_quoted_template)
    assert single_quoted_result.data == render_data(single_quoted_template)
    assert single_quoted_result.text == render_text(single_quoted_template)


def test_yaml_public_exports_are_standardized() -> None:
    assert {
        "all": yaml_tstring.__all__,
        "parse_identity": yaml_tstring.TemplateParseError is TemplateParseError,
        "semantic_identity": yaml_tstring.TemplateSemanticError
        is TemplateSemanticError,
        "unrepr_identity": yaml_tstring.UnrepresentableValueError
        is UnrepresentableValueError,
        "render_result_type_identity": yaml_tstring.RenderResult is CoreRenderResult,
        "imported_render_result_type_identity": RenderResult is CoreRenderResult,
        "template_error_identity": yaml_tstring.TemplateError is TemplateError,
        "render_data_identity": yaml_tstring.render_data is render_data,
        "render_result_identity": yaml_tstring.render_result is render_result,
        "render_text_identity": yaml_tstring.render_text is render_text,
    } == snapshot(
        {
            "all": [
                "RenderResult",
                "TemplateError",
                "TemplateParseError",
                "TemplateSemanticError",
                "UnrepresentableValueError",
                "YamlProfile",
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
