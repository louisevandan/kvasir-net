#!/usr/bin/env python3
"""Execute the sealed I1 closed-loop corpus with the I0 remote evidence collector."""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import tempfile
from pathlib import Path


DIRECTORY = Path(__file__).resolve().parent


def load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def validate_i1(manifest: dict, config: dict, seal: dict, spec: dict,
                config_path: Path) -> None:
    quality = spec["workload"]["modes"]["quality"]
    collector = load_module("i1_remote_collector", DIRECTORY / "run-integrity-i0.py")
    collector.validate_manifest(manifest)
    if manifest["local_config"] != str(config_path.resolve()):
        raise ValueError("I1 local config path differs")
    if seal.get("config_sha256") != digest(config_path):
        raise ValueError("I1 config seal differs")
    if len(config.get("prompts", [])) != quality["requests"] or seal.get("requests") != quality["requests"]:
        raise ValueError("I1 corpus request count differs")
    if (config.get("max_in_flight") != 1 or config.get("waves") != quality["waves"]
            or config.get("timeout_ms") != quality["timeout_ms"]
            or config.get("request_timeout_ms") != seal.get("request_timeout_ms")):
        raise ValueError("I1 closed-loop schedule differs")
    barrier_names = ("pre_inference_hold_ms", "inference_start_hold_ms", "post_inference_hold_ms")
    expected = seal.get("observation_barriers")
    if (not isinstance(expected, dict) or
            {name: config.get(name) for name in barrier_names} != expected or
            {name: quality.get(name) for name in barrier_names} != expected or
            any(expected.get(name) != 15_000 for name in barrier_names)):
        raise ValueError("I1 execution observation barriers differ")
    if len(config.get("response_processors", [])) != quality["requests"]:
        raise ValueError("I1 response processor coverage differs")
    for row, processor, prompt in zip(seal["cases"], config["response_processors"], config["prompts"]):
        if (processor != (None if row["class"] == "short" else "engineering_power_v1")
                or hashlib.sha256(prompt.encode()).hexdigest() != row["prompt_sha256"]):
            raise ValueError("I1 prompt or processor binding differs")
    if manifest["driver_timeout_seconds"] < quality["timeout_ms"] // 1000 + 120:
        raise ValueError("I1 driver bound is shorter than the sealed deadline")
    if manifest.get("artifact_read_timeout_seconds") != 900:
        raise ValueError("I1 artifact transfer deadline differs")


def run(args: argparse.Namespace) -> dict:
    manifest = json.loads(args.manifest.read_text())
    config_path = Path(manifest["local_config"]).resolve()
    config = json.loads(config_path.read_text())
    seal = json.loads(args.seal.read_text())
    spec = json.loads(args.spec.read_text())
    if seal.get("spec_sha256") != digest(args.spec):
        raise ValueError("I1 H0 spec seal differs")
    validate_i1(manifest, config, seal, spec, config_path)
    collector = load_module("i1_remote_collector", DIRECTORY / "run-integrity-i0.py")
    result = collector.run(manifest, args.output)
    artifact = json.loads((args.output / "artifact.json").read_text())
    h1 = load_module("i1_quality_judge", DIRECTORY / "judge-h1-quality.py")
    judgment = h1.evaluate(artifact, seal)
    (args.output / "h1-judgment.json").write_text(json.dumps(judgment, indent=2) + "\n")
    raw_path = args.output / "raw.json"
    raw = json.loads(raw_path.read_text())
    raw["schema"] = "p4.release-a.integrity-i1-raw.v1"
    (args.output / "i1-raw.json").write_text(json.dumps(raw, indent=2) + "\n")
    result.update(i1_passed=result["passed"] and judgment["passed"]
                  and len(judgment["rows"]) == seal["requests"],
                  cases=len(judgment["rows"]))
    return result


def self_test() -> None:
    barriers = {name: 15_000 for name in
                ("pre_inference_hold_ms", "inference_start_hold_ms", "post_inference_hold_ms")}
    names = ["driver", "config", "route", "controller-script", "controller-config"] + [
        f"{kind}-{host}" for host in ("spark", "mac20", "mac21")
        for kind in ("observer", "observer-config", "cleanup-script", "cleanup-config")]
    prompts = ["short", "long"]
    config = {"prompts": prompts, "response_processors": [None, "engineering_power_v1"],
              "max_in_flight": 1, "waves": [{"after_ms": 0, "count": 2}],
              "timeout_ms": 100_000, "request_timeout_ms": [40_000, 60_000], **barriers}
    spec = {"workload": {"modes": {"quality": {"requests": 2, "waves": config["waves"],
                                           "timeout_ms": 100_000, **barriers}}}}
    seal = {"requests": 2, "request_timeout_ms": config["request_timeout_ms"],
            "observation_barriers": barriers, "cases": [
                {"class": class_name, "prompt_sha256": hashlib.sha256(prompt.encode()).hexdigest()}
                for class_name, prompt in zip(("short", "long"), prompts)]}
    with tempfile.TemporaryDirectory() as temporary:
        path = Path(temporary) / "config.json"
        path.write_text(json.dumps(config))
        seal["config_sha256"] = digest(path)
        manifest = {"local_config": str(path.resolve()), "driver_timeout_seconds": 220,
                    "artifact_read_timeout_seconds": 900,
                    "hosts": [{"host": host} for host in ("spark", "mac20", "mac21")],
                    "bindings": [{"name": name, "command": ["hash"], "sha256": "0" * 64}
                                 for name in names],
                    "controller_preflight": {"host": "windows-controller", "command": ["inspect"],
                        "owned": ["mac20-return", "mac20-next", "mac21-return", "mac21-previous"]}}
        validate_i1(manifest, config, seal, spec, path)
        for changed_manifest, changed_config, changed_seal in (
                (manifest, {**config, "post_inference_hold_ms": 0}, seal),
                ({**manifest, "driver_timeout_seconds": 119}, config, seal),
                ({**manifest, "artifact_read_timeout_seconds": 60}, config, seal),
                (manifest, {**config, "response_processors": [None, None]}, seal),
                (manifest, config, {**seal, "config_sha256": "0" * 64}),
                (manifest, {**config, "prompts": ["short"]}, seal),
        ):
            try:
                validate_i1(changed_manifest, changed_config, changed_seal, spec, path)
            except ValueError:
                pass
            else:
                raise AssertionError("weakened I1 execution config was accepted")
    print(json.dumps({"passed": True, "tests": 7}, separators=(",", ":")))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--seal", type=Path)
    parser.add_argument("--spec", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test(); return
    if any(value is None for value in (args.manifest, args.seal, args.spec, args.output)):
        parser.error("manifest, seal, spec and output are required")
    result = run(args)
    print(json.dumps(result, separators=(",", ":")))
    if not result["i1_passed"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
