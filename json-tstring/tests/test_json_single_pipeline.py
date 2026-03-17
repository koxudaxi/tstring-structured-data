from __future__ import annotations

import pytest

from json_tstring import TemplateParseError, render_result, render_text


class BadStringValue:
    def __str__(self) -> str:
        raise ValueError("cannot stringify")


def test_json_result_prefers_earlier_native_payload_errors() -> None:
    broken = "{bad json}"
    bad = BadStringValue()
    template = t'{{"value": {broken!s}, "tail": "{bad}"}}'

    with pytest.raises(TemplateParseError, match="invalid formatted JSON payload"):
        render_text(template)

    with pytest.raises(TemplateParseError, match="invalid formatted JSON payload"):
        render_result(template)
