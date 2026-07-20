#!/usr/bin/env python3
"""Single-source release version for the linkcpp repository."""
from pathlib import Path


_VERSION_FILE = Path(__file__).resolve().parent.parent / "VERSION"


def read_release_version() -> str:
    """Return the tracked repository release version.

    A missing or empty file is a packaging error: silently substituting a
    version would reintroduce inconsistent compatibility reports.
    """
    try:
        version = _VERSION_FILE.read_text(encoding="utf-8").strip()
    except OSError as exc:
        raise RuntimeError(f"linkcpp VERSION file is unavailable: {_VERSION_FILE}") from exc
    if not version:
        raise RuntimeError(f"linkcpp VERSION file is empty: {_VERSION_FILE}")
    return version


RELEASE_VERSION = read_release_version()
