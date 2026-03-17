from __future__ import annotations

from datetime import date, datetime, time

type JsonScalar = None | bool | int | float | str
type JsonValue = JsonScalar | list[JsonValue] | dict[str, JsonValue]

type TomlScalar = bool | int | float | str | date | time | datetime
type TomlValue = TomlScalar | list[TomlValue] | dict[str, TomlValue]

type YamlScalar = None | bool | int | float | str
type YamlKey = YamlScalar | tuple[YamlKey, ...] | frozenset[tuple[YamlKey, YamlKey]]
type YamlValue = YamlScalar | list[YamlValue] | dict[YamlKey, YamlValue]
