"""Inspect and manually advance the pinned llama.cpp compatibility track.

The official upstream and the linkcpp adapter fork are deliberately separate.
Only adapter commits can be applied automatically because ring support depends
on a reviewed adapter rebase; an official HEAD must never silently replace it.
"""

from __future__ import annotations

import os
import argparse
import json
import subprocess
from typing import Any, Callable


OFFICIAL_URL = os.environ.get("LINKCPP_LLAMA_UPSTREAM_URL", "https://github.com/ggml-org/llama.cpp.git")
# The adapter fork is deployment-specific — set LINKCPP_LLAMA_ADAPTER_URL to your
# own linkcpp-llama.cpp fork. No default so a private fork URL is never baked in.
ADAPTER_URL = os.environ.get("LINKCPP_LLAMA_ADAPTER_URL", "")


def _repo_root() -> str:
    return os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))


def _llama_dir() -> str:
    return os.environ.get("LINKCPP_LLAMA_SOURCE_DIR", os.path.join(_repo_root(), "external", "llama.cpp"))


def _run(args: list[str], *, cwd: str | None = None, timeout: int = 30) -> str:
    return subprocess.check_output(args, cwd=cwd, text=True, stderr=subprocess.STDOUT, timeout=timeout).strip()


def _head(url: str, runner: Callable[..., str] = _run) -> str:
    output = runner(["git", "ls-remote", url, "HEAD"], timeout=30)
    line = next((line for line in output.splitlines() if line.strip()), "")
    sha = line.split()[0] if line else ""
    if len(sha) != 40:
        raise RuntimeError(f"cannot resolve remote HEAD: {url}")
    return sha


def update_status(runner: Callable[..., str] = _run) -> dict[str, Any]:
    source = _llama_dir()
    if not os.path.isdir(source):
        return {"supported": False, "reason": "llama.cpp source checkout is not present", "source_dir": source}
    try:
        current = runner(["git", "rev-parse", "HEAD"], cwd=source)
        dirty = bool(runner(["git", "status", "--porcelain"], cwd=source))
        official = _head(OFFICIAL_URL, runner)
        adapter = _head(ADAPTER_URL, runner)
        return {
            "supported": True,
            "source_dir": source,
            "current": current,
            "official_head": official,
            "adapter_head": adapter,
            "dirty": dirty,
            "adapter_update_available": current != adapter,
            "official_differs_from_adapter": official != adapter,
            "apply_track": "adapter",
            "note": "official updates require a reviewed adapter rebase before they can be applied",
        }
    except Exception as exc:
        return {"supported": False, "reason": str(exc), "source_dir": source}


def apply_adapter_update(expected_current: str, target: str, runner: Callable[..., str] = _run) -> dict[str, Any]:
    status = update_status(runner)
    if not status.get("supported"):
        raise RuntimeError(status.get("reason") or "llama.cpp update is unavailable")
    if status["dirty"]:
        raise RuntimeError("llama.cpp checkout has local changes")
    if expected_current != status["current"]:
        raise RuntimeError("llama.cpp version changed after the update check")
    if target != status["adapter_head"]:
        raise RuntimeError("target is not the current compatible adapter HEAD")
    source = status["source_dir"]
    runner(["git", "fetch", ADAPTER_URL, target], cwd=source, timeout=120)
    try:
        runner(["git", "merge-base", "--is-ancestor", expected_current, target], cwd=source, timeout=30)
    except Exception as exc:
        raise RuntimeError(
            "compatible adapter HEAD is not a fast-forward from the pinned revision; manual rebase review is required"
        ) from exc
    runner(["git", "checkout", "--detach", target], cwd=source, timeout=60)
    return {**update_status(runner), "updated_from": expected_current, "updated_to": target}


def main() -> int:
    parser = argparse.ArgumentParser(description="Check or update linkcpp's pinned llama.cpp adapter")
    parser.add_argument("--apply-compatible", action="store_true",
                        help="advance to the current compatible adapter HEAD")
    parser.add_argument("--yes", action="store_true", help="skip the interactive confirmation")
    args = parser.parse_args()
    status = update_status()
    if not args.apply_compatible:
        print(json.dumps(status, indent=2))
        return 0 if status.get("supported") else 1
    if not status.get("supported"):
        raise SystemExit(status.get("reason") or "update unavailable")
    if not status.get("adapter_update_available"):
        print(json.dumps(status, indent=2))
        return 0
    if not args.yes:
        answer = input(f"Update adapter {status['current'][:12]} -> {status['adapter_head'][:12]}? [y/N] ")
        if answer.strip().lower() not in ("y", "yes"):
            return 2
    print(json.dumps(apply_adapter_update(status["current"], status["adapter_head"]), indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
