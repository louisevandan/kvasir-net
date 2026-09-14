"""Explicit immutable allocation ceiling for one frame."""

from dataclasses import dataclass
import sys


@dataclass(frozen=True, slots=True)
class FrameLimits:
    max_payload_bytes: int

    def __post_init__(self) -> None:
        if type(self.max_payload_bytes) is not int or not 0 < self.max_payload_bytes <= min(sys.maxsize, 2**64 - 1):
            raise ValueError("max_payload_bytes must be a positive platform-sized integer")
