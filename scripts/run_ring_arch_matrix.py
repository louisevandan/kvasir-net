#!/usr/bin/env python3
"""Generate llama.cpp tiny architecture fixtures and compare stock vs. two-stage ring."""

from __future__ import annotations

import argparse
import concurrent.futures
import json
import math
import os
import re
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any


ARCH_PATTERN = re.compile(r'\{\s*LLM_ARCH_[A-Z0-9_]+,\s*"([^"]+)"\s*\}')
ARCH_ENTRY_PATTERN = re.compile(r'\{\s*(LLM_ARCH_[A-Z0-9_]+),\s*"([^"]+)"\s*\}')
MODEL_FACTORY_PATTERN = re.compile(
    r"case\s+(LLM_ARCH_[A-Z0-9_]+)\s*:\s*"
    r"return\s+new\s+llama_model_[a-zA-Z0-9_]+\(params\);"
)
EMBEDDING_ARCHES = {
    "bert", "modern-bert", "nomic-bert", "nomic-bert-moe", "neo-bert",
    "jina-bert-v2", "jina-bert-v3", "eurobert", "t5encoder",
    "gemma-embedding", "llama-embed", "pangu-embedded",
}


def http_json(url: str, body: dict[str, Any] | None = None, timeout: float = 30.0):
    data = None if body is None else json.dumps(body).encode("utf-8")
    request = urllib.request.Request(
        url, data=data, headers={"Content-Type": "application/json"},
        method="GET" if body is None else "POST",
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return json.loads(response.read().decode("utf-8"))


def wait_health(process: subprocess.Popen, base_url: str, timeout: float) -> None:
    deadline = time.monotonic() + timeout
    error = "server did not become healthy"
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"server exited with code {process.returncode}")
        try:
            result = http_json(base_url + "/health", timeout=2.0)
            if result.get("status") in ("ok", "no slot available"):
                return
        except Exception as exc:
            error = str(exc)
        time.sleep(0.2)
    raise RuntimeError(error)


def stop(process: subprocess.Popen | None) -> None:
    if process is None or process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=10)


def read_model(path: Path) -> dict[str, Any]:
    from controller.planner import read_model as read
    return read(str(path))


def request_output(base_url: str, arch: str, timeout: float) -> dict[str, Any]:
    tokens = [1, 7, 11, 3]
    if arch in EMBEDDING_ARCHES:
        body = http_json(
            base_url + "/v1/embeddings", {"input": [tokens], "encoding_format": "float"}, timeout,
        )
        data = body.get("data") or []
        vector = list((data[0] if data else {}).get("embedding") or [])
        if not vector:
            raise RuntimeError("embedding response is empty")
        return {"kind": "embedding", "values": vector}
    body = http_json(base_url + "/completion", {
        "prompt": tokens,
        "n_predict": 1,
        "temperature": 0.0,
        "seed": 1,
        "return_tokens": True,
        "cache_prompt": False,
    }, timeout)
    generated = list(body.get("tokens") or [])
    if len(generated) != 1:
        raise RuntimeError(f"completion did not return one token: {generated}")
    return {"kind": "completion", "tokens": generated}


def compare_output(stock: dict[str, Any], ring: dict[str, Any]) -> tuple[bool, dict[str, Any]]:
    if stock.get("kind") != ring.get("kind"):
        return False, {"reason": "output kinds differ"}
    if stock["kind"] == "completion":
        return stock["tokens"] == ring["tokens"], {
            "stock_tokens": stock["tokens"], "ring_tokens": ring["tokens"],
        }
    left = stock["values"]
    right = ring["values"]
    if len(left) != len(right) or not left:
        return False, {"reason": "embedding lengths differ", "stock": len(left), "ring": len(right)}
    square_error = sum((float(a) - float(b)) ** 2 for a, b in zip(left, right))
    square_reference = sum(float(a) ** 2 for a in left)
    nmse = square_error / max(square_reference, sys.float_info.min)
    return math.isfinite(nmse) and nmse <= 1e-6, {
        "values": len(left), "nmse": nmse,
        "stock_sum": sum(map(float, left)), "ring_sum": sum(map(float, right)),
    }


def launch(log_path: Path, command: list[str], env: dict[str, str]) -> subprocess.Popen:
    log_path.parent.mkdir(parents=True, exist_ok=True)
    log = log_path.open("wb")
    try:
        return subprocess.Popen(
            command, env=env, stdin=subprocess.DEVNULL, stdout=log,
            stderr=subprocess.STDOUT,
        )
    finally:
        log.close()


def launch_control(log_path: Path, command: list[str], env: dict[str, str]) -> subprocess.Popen:
    log_path.parent.mkdir(parents=True, exist_ok=True)
    log = log_path.open("wb")
    try:
        return subprocess.Popen(
            command, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log,
        )
    finally:
        log.close()


def evaluate_model(
    model: Path, arch: str, args, index: int, report_dir: Path,
) -> dict[str, Any]:
    metadata = read_model(model)
    layers = int(metadata["n_layer"])
    if layers < 2:
        raise RuntimeError("fixture has fewer than two executable layers")
    boundary = max(1, layers // 2)
    if boundary >= layers:
        boundary = layers - 1
    first_stage, last_stage = args.port_base + index * 2, args.port_base + index * 2 + 1
    env = dict(os.environ)
    if args.cuda_visible_devices is not None:
        env["CUDA_VISIBLE_DEVICES"] = args.cuda_visible_devices
    common_node = [
        str(args.node), "--model", str(model), "--gpu-layers", str(args.gpu_layers),
        "--ctx", "128", "--parallel", "1",
    ]
    ring_first = ring_last = None
    started = time.monotonic()
    try:
        ring_last = launch(report_dir / f"{arch}-{model.stem}-last.log", [
            *common_node, "--layers", f"{boundary}:{layers}",
            "--role", "last", "--listen", str(last_stage),
            "--next", f"127.0.0.1:{first_stage}",
        ], env)
        ring_first = launch_control(
            report_dir / f"{arch}-{model.stem}-first.log",
            [
                *common_node, "--layers", f"0:{boundary}", "--role", "first",
                "--listen", str(first_stage), "--next", f"127.0.0.1:{last_stage}",
            ], env,
        )
        golden = json.loads(model.with_suffix(".golden.json").read_text(encoding="utf-8"))
        from controller.proxy.control import infer_tokens_process
        with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
            future = pool.submit(infer_tokens_process, ring_first, golden["tokens"], index + 1)
            try:
                actual = future.result(timeout=args.request_timeout)
            except concurrent.futures.TimeoutError:
                stop(ring_first)
                stop(ring_last)
                raise RuntimeError("ring token evaluation timed out")
        matches = int(actual["token"]) == int(golden["argmax"])
        evidence = {
            "stock_argmax": int(golden["argmax"]), "ring_argmax": int(actual["token"]),
        }
        return {
            "status": "pass" if matches else "mismatch",
            "architecture": arch,
            "model": model.name,
            "layers": layers,
            "boundary": boundary,
            "elapsed_s": round(time.monotonic() - started, 3),
            "evidence": evidence,
        }
    except (OSError, RuntimeError, urllib.error.URLError, json.JSONDecodeError) as exc:
        return {
            "status": "error", "architecture": arch, "model": model.name,
            "layers": layers, "boundary": boundary,
            "elapsed_s": round(time.monotonic() - started, 3), "error": str(exc),
        }
    finally:
        stop(ring_first)
        stop(ring_last)


def architectures(arch_source: Path, model_source: Path, selected: str) -> list[str]:
    names = dict(ARCH_ENTRY_PATTERN.findall(arch_source.read_text(encoding="utf-8")))
    factories = MODEL_FACTORY_PATTERN.findall(model_source.read_text(encoding="utf-8"))
    known = [names[enum] for enum in factories if enum in names]
    if not selected:
        return known
    requested = [item.strip() for item in selected.split(",") if item.strip()]
    unknown = sorted(set(requested) - set(known))
    if unknown:
        raise ValueError("unknown architectures: " + ", ".join(unknown))
    return requested


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--fixtures", required=True, type=Path)
    parser.add_argument("--node", required=True, type=Path)
    parser.add_argument("--llama-arch-source", required=True, type=Path)
    parser.add_argument("--llama-model-source", required=True, type=Path)
    parser.add_argument("--work-dir", required=True, type=Path)
    parser.add_argument("--report", required=True, type=Path)
    parser.add_argument("--architectures", default="")
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--gpu-layers", type=int, default=0)
    parser.add_argument("--cuda-visible-devices")
    parser.add_argument("--port-base", type=int, default=56000)
    parser.add_argument("--start-timeout", type=float, default=120.0)
    parser.add_argument("--request-timeout", type=float, default=120.0)
    parser.add_argument("--keep-models", action="store_true")
    args = parser.parse_args()
    targets = architectures(args.llama_arch_source, args.llama_model_source, args.architectures)
    args.work_dir.mkdir(parents=True, exist_ok=True)
    report_dir = args.report.parent / (args.report.stem + "-logs")
    report_dir.mkdir(parents=True, exist_ok=True)
    results: list[dict[str, Any]] = []
    model_index = 0
    for arch in targets:
        arch_dir = args.work_dir / re.sub(r"[^a-zA-Z0-9_.-]", "_", arch)
        shutil.rmtree(arch_dir, ignore_errors=True)
        arch_dir.mkdir(parents=True)
        generated = subprocess.run(
            [str(args.fixtures), "--arch", arch, "--seed", str(args.seed), "--out", str(arch_dir)],
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
        )
        models = sorted(arch_dir.glob("*.gguf"))
        if generated.returncode != 0 or not models:
            results.append({
                "status": "fixture_missing", "architecture": arch,
                "generator_exit": generated.returncode,
                "generator_output": generated.stdout[-4000:],
            })
        for model in models:
            results.append(evaluate_model(model, arch, args, model_index, report_dir))
            model_index += 1
        if not args.keep_models:
            shutil.rmtree(arch_dir, ignore_errors=True)
        report = {
            "schema": 1, "seed": args.seed, "architectures_requested": targets,
            "results": results,
            "summary": {
                status: sum(item["status"] == status for item in results)
                for status in ("pass", "mismatch", "error", "fixture_missing")
            },
        }
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(json.dumps(results[-1], sort_keys=True), flush=True)
    return 0 if results and all(item["status"] == "pass" for item in results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
