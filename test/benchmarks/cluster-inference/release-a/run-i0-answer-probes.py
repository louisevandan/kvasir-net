#!/usr/bin/env python3
"""Run a presealed, one-load Qwen122B answer-cause diagnostic on one remote host."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import signal
import socket
import subprocess
import time
import urllib.error
import urllib.request
from pathlib import Path


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def write(path: Path, value: object) -> None:
    with path.open("x", encoding="utf-8") as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2)
        stream.write("\n")


def validate_inputs(inputs: Path, model: Path) -> dict:
    manifest = json.loads((inputs / "manifest.json").read_text(encoding="utf-8"))
    if manifest.get("schema") != "p4.release-a.i0-answer-cause.v1" or len(manifest.get("probes", [])) != 6:
        raise ValueError("probe manifest differs")
    if manifest.get("model_path") != str(model):
        raise ValueError("model path differs from tokenization")
    if manifest.get("order") != [row["id"] for row in manifest["probes"]]:
        raise ValueError("probe order differs")
    for row in manifest["probes"]:
        name = row["id"]
        prompt = (inputs / f"{name}.prompt.txt").read_bytes()
        oracle = (inputs / f"{name}.oracle.json").read_bytes()
        tokens = (inputs / f"{name}.tokens.bin").read_bytes()
        if sha(prompt) != row["prompt_sha256"] or sha(oracle) != row["oracle_sha256"]:
            raise ValueError(f"probe prompt/oracle differs: {name}")
        if not tokens or len(tokens) != row["tokens"] * 4 or sha(tokens) != row["token_ids_sha256"]:
            raise ValueError(f"probe token IDs differ: {name}")
    return manifest


def health(port: int) -> bool:
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{port}/health", timeout=5) as response:
            return json.load(response).get("status") == "ok"
    except (OSError, ValueError, urllib.error.HTTPError):
        return False


def free_port(port: int) -> bool:
    with socket.socket() as probe:
        return probe.connect_ex(("127.0.0.1", port)) != 0


def run(inputs: Path, output: Path, server_binary: Path, model: Path,
        binary_sha256: str, port: int) -> None:
    if output.exists():
        raise FileExistsError(output)
    if sha(server_binary.read_bytes()) != binary_sha256:
        raise ValueError("server binary differs")
    dynamic = Path("/proc/sys/net/ipv4/ip_local_port_range").read_text().split()
    first, last = map(int, dynamic)
    if first <= port <= last or not free_port(port):
        raise ValueError("server port is dynamic or occupied")
    manifest = validate_inputs(inputs, model)
    output.mkdir()
    argv = [str(server_binary), "--model", str(model), "--host", "127.0.0.1",
            "--port", str(port), "--ctx-size", "131072", "--parallel", "1",
            "--batch-size", "128", "--ubatch-size", "64", "--n-gpu-layers", "999",
            "--flash-attn", "on", "--load-mode", "none", "--cache-type-k", "f16",
            "--cache-type-v", "f16", "--threads", "20", "--threads-batch", "20"]
    runtime = {"schema": "p4.release-a.i0-answer-cause-run.v1", "server_binary_sha256": binary_sha256,
               "model": str(model), "argv": argv, "port": port,
               "manifest_sha256": sha((inputs / "manifest.json").read_bytes()), "requests": []}
    environment = {**os.environ, "LD_LIBRARY_PATH": str(server_binary.parent) + ":" + os.environ.get("LD_LIBRARY_PATH", "")}
    server = None
    def interrupt(_number, _frame):
        raise KeyboardInterrupt("diagnostic runner interrupted")
    previous = signal.signal(signal.SIGTERM, interrupt)
    try:
        with (output / "server.log").open("xb") as log:
            server = subprocess.Popen(argv, stdout=log, stderr=subprocess.STDOUT, env=environment)
            runtime["server_pid"] = server.pid
            write(output / "runtime.json", runtime)
            deadline = time.monotonic() + 1800
            while not health(port):
                if server.poll() is not None:
                    raise RuntimeError(f"server exited while loading: {server.returncode}")
                if time.monotonic() >= deadline:
                    raise TimeoutError("server load deadline")
                time.sleep(5)
            print(json.dumps({"server_ready": True, "pid": server.pid}), flush=True)
            for row in manifest["probes"]:
                name = row["id"]
                prompt = (inputs / f"{name}.prompt.txt").read_text(encoding="utf-8")
                request = {"prompt": prompt, "n_predict": 2048, "temperature": 0,
                           "top_p": 1, "top_k": 0, "min_p": 0, "repeat_penalty": 1,
                           "seed": 20260916, "stream": False, "cache_prompt": False}
                payload = json.dumps(request).encode("utf-8")
                started = int(time.time() * 1000)
                start = time.monotonic()
                post = urllib.request.Request(f"http://127.0.0.1:{port}/completion", payload,
                                              {"Content-Type": "application/json"})
                with urllib.request.urlopen(post, timeout=2000) as response:
                    data = json.load(response)
                elapsed = int((time.monotonic() - start) * 1000)
                write(output / f"{name}.response.json", data)
                runtime["requests"].append({"id": name, "started_unix_ms": started,
                                            "elapsed_ms": elapsed,
                                            "response_sha256": sha((output / f"{name}.response.json").read_bytes())})
                (output / "runtime.json").write_text(json.dumps(runtime, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
                print(json.dumps({"case": name, "elapsed_ms": elapsed,
                                  "tokens_evaluated": data.get("tokens_evaluated"),
                                  "tokens_predicted": data.get("tokens_predicted"),
                                  "stop_type": data.get("stop_type")}), flush=True)
    except BaseException as error:
        write(output / "failure.json", {"error": repr(error), "completed": len(runtime["requests"])})
        raise
    finally:
        signal.signal(signal.SIGTERM, previous)
        if server is not None:
            if server.poll() is None:
                server.terminate()
                try:
                    server.wait(timeout=45)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait(timeout=15)
            cleanup = {"server_pid": server.pid, "server_exit": server.returncode,
                       "port_free": free_port(port), "server_alive": server.poll() is None}
            write(output / "cleanup.json", cleanup)
            print(json.dumps({"cleanup": cleanup}), flush=True)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--inputs", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--server-binary", type=Path, required=True)
    parser.add_argument("--model", type=Path, required=True)
    parser.add_argument("--binary-sha256", required=True)
    parser.add_argument("--port", type=int, default=62005)
    args = parser.parse_args()
    run(args.inputs, args.output, args.server_binary, args.model, args.binary_sha256, args.port)


if __name__ == "__main__":
    main()
