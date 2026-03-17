from __future__ import annotations

import json
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import TypedDict, cast

from ._types import JsonValue


class _ConformanceProfile(TypedDict):
    manifest_path: str


class _ConformanceProfileIndex(TypedDict):
    supported_profiles: list[str]
    profiles: dict[str, _ConformanceProfile]


class _ConformanceProvenance(TypedDict):
    source: str
    snapshot: str


class _ConformanceCaseRecord(TypedDict, total=False):
    case_id: str
    spec_ref: str
    expected: str
    execution_layer: str
    input_path: str
    note: str
    expected_json_path: str
    classification: str


class _ConformanceManifest(TypedDict):
    spec_title: str
    claim_status: str
    provenance: _ConformanceProvenance
    cases: list[_ConformanceCaseRecord]


@dataclass(frozen=True)
class ConformanceCase:
    format_name: str
    case_id: str
    spec_ref: str
    expected: str
    execution_layer: str
    input_path: str
    note: str
    expected_json_path: str | None = None
    classification: str | None = None

    def input_text(self) -> str:
        with self.base_dir.joinpath(self.input_path).open(
            "r", encoding="utf-8", newline=""
        ) as handle:
            return handle.read()

    def expected_json(self) -> JsonValue | None:
        if self.expected_json_path is None:
            return None
        return cast(
            JsonValue,
            json.loads(
                self.base_dir.joinpath(self.expected_json_path).read_text(
                    encoding="utf-8"
                )
            ),
        )

    @property
    def base_dir(self) -> Path:
        return _repo_root() / "conformance" / self.format_name


@dataclass(frozen=True)
class ConformanceSuite:
    format_name: str
    profile: str
    spec_title: str
    claim_status: str
    source: str
    snapshot: str
    cases: tuple[ConformanceCase, ...]

    def iter_cases(self, layer: str) -> tuple[ConformanceCase, ...]:
        return tuple(
            case for case in self.cases if case.execution_layer in {layer, "both"}
        )


def load_conformance_suite(format_name: str, profile: str) -> ConformanceSuite:
    format_root = _repo_root() / "conformance" / format_name
    profile_index_path = format_root / "profiles.toml"
    profile_index = cast(
        _ConformanceProfileIndex,
        tomllib.loads(profile_index_path.read_text(encoding="utf-8")),
    )
    supported_profiles = tuple(profile_index["supported_profiles"])
    if profile not in supported_profiles:
        supported_profile_list = ", ".join(repr(value) for value in supported_profiles)
        raise ValueError(
            f"Unsupported {format_name} conformance profile {profile!r}. "
            f"Supported profiles: {supported_profile_list}."
        )
    manifest_path = format_root / profile_index["profiles"][profile]["manifest_path"]
    manifest = cast(
        _ConformanceManifest,
        tomllib.loads(manifest_path.read_text(encoding="utf-8")),
    )
    return ConformanceSuite(
        format_name=format_name,
        profile=profile,
        spec_title=manifest["spec_title"],
        claim_status=manifest["claim_status"],
        source=manifest["provenance"]["source"],
        snapshot=manifest["provenance"]["snapshot"],
        cases=tuple(
            ConformanceCase(format_name=format_name, **case)
            for case in manifest["cases"]
        ),
    )


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[3]
