from __future__ import annotations

from typing import Literal

type JsonProfile = Literal["rfc8259"]
type TomlProfile = Literal["1.0", "1.1"]
type YamlProfile = Literal["1.2.2"]

DEFAULT_JSON_PROFILE: JsonProfile = "rfc8259"
DEFAULT_TOML_PROFILE: TomlProfile = "1.1"
DEFAULT_YAML_PROFILE: YamlProfile = "1.2.2"


def resolve_json_profile(profile: JsonProfile | str | None) -> JsonProfile:
    if profile is None or profile == "rfc8259":
        return "rfc8259"
    raise ValueError(
        f"Unsupported JSON profile {profile!r}. Supported profiles: 'rfc8259'."
    )


def resolve_toml_profile(profile: TomlProfile | str | None) -> TomlProfile:
    if profile is None:
        return DEFAULT_TOML_PROFILE
    if profile == "1.0":
        return "1.0"
    if profile == "1.1":
        return "1.1"
    raise ValueError(
        f"Unsupported TOML profile {profile!r}. Supported profiles: '1.0', '1.1'."
    )


def resolve_yaml_profile(profile: YamlProfile | str | None) -> YamlProfile:
    if profile is None or profile == "1.2.2":
        return "1.2.2"
    raise ValueError(
        f"Unsupported YAML profile {profile!r}. Supported profiles: '1.2.2'."
    )
