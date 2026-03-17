from __future__ import annotations

from dataclasses import dataclass
from enum import Enum


class SlotContext(str, Enum):
    VALUE = "value"
    KEY = "key"
    STRING_FRAGMENT = "string_fragment"
    UNSUPPORTED = "unsupported"


@dataclass(slots=True)
class Slot:
    id: int


@dataclass(slots=True)
class FragmentGroup:
    start_slot: int
    end_slot: int
    start_offset: int
    end_offset: int
