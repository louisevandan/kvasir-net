#!/usr/bin/env python3
"""Seal visible I0 answer-cause probes and classify their exact responses."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
import re
import struct
import subprocess
import tempfile
from pathlib import Path


CASES = ("case-04", "case-06")
SUFFIX = "<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n"
FACT_KEYS = ("id", "revision", "amps", "milliohms", "hours", "pressure")
SETTING = {"seed": 20260916, "temperature": 0, "top_k": 0, "top_p": 1,
           "min_p": 0, "repeat_penalty": 1, "max_tokens": 2048,
           "speculative.types": "none"}


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def put(path: Path, data: bytes) -> None:
    with path.open("xb") as stream:
        stream.write(data)


def json_bytes(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=False, indent=2) + "\n").encode("utf-8")


def source_line(prompt: str, fact: dict) -> str:
    matches = re.findall(rf"^\[{re.escape(fact['id'])}\].*$", prompt, flags=re.MULTILINE)
    if len(matches) != 1:
        raise ValueError(f"source ID is not unique: {fact['id']}")
    line = matches[0]
    for value, label in ((fact["revision"], "revision "), (fact["amps"], "RMS current: "),
                         (fact["milliohms"], "resistance: "), (fact["hours"], "duration: "),
                         (fact["pressure"], "pressure: ")):
        if not re.search(rf"{re.escape(label)}{value}(?:\D|$)", line):
            raise ValueError(f"source fact differs: {fact['id']} {label}")
    return line


def prepare(corpus: Path, prior: Path, output: Path) -> dict:
    if output.exists():
        raise FileExistsError(output)
    output.mkdir()
    probes = []
    for case in CASES:
        original_file = corpus / f"{case}.prompt.txt"
        original = original_file.read_text(encoding="utf-8")
        facts = json.loads((corpus / f"{case}.facts.json").read_text(encoding="utf-8"))
        oracle = json.loads((corpus / f"{case}.oracle.json").read_text(encoding="utf-8"))
        prior_file = prior / f"{case}.response.json"
        previous = json.loads(prior_file.read_text(encoding="utf-8"))
        if previous.get("prompt") != original or not original.endswith(SUFFIX):
            raise ValueError(f"prior input identity/template differs: {case}")
        marker = "\n\nConnect the source facts for "
        if original.count(marker) != 1:
            raise ValueError(f"original task boundary differs: {case}")
        record_prefix = original.split(marker, 1)[0]
        selected = [source_line(original, fact) for fact in facts]
        wanted = [{key: fact[key] for key in FACT_KEYS} for fact in facts]
        ids = ", ".join(fact["id"] for fact in facts)
        extract = (f"For {ids}, in that order, transcribe only the source values. Return only JSON with "
                   'a "facts" array; each object has exactly "id", "revision", "amps", '
                   '"milliohms", "hours", "pressure". Do not calculate or infer values.')
        calc = ("Compute power_mW = amps squared times milliohms and energy_mWh = power_mW "
                "times hours for the three records. pressure_alarm is true only when pressure exceeds "
                '120 kPa. Return only JSON with "rows" in source order, each containing exactly '
                '"id", "revision", "power_mW", "energy_mWh", "pressure_alarm", and '
                '"temperature_measured": false. Use integer arithmetic.')
        trace = (f"For {ids}, in that order, first transcribe the source values, then calculate. "
                 'Return only one JSON object with "facts", "rows", and "temperature_measured". '
                 'Each "facts" object has exactly "id", "revision", "amps", "milliohms", '
                 '"hours", "pressure". Each "rows" object has exactly "id", "revision", '
                 '"power_mW", "energy_mWh", "pressure_alarm". Compute power_mW = amps squared '
                 'times milliohms, energy_mWh = power_mW times hours, pressure_alarm = pressure > '
                 '120 kPa. temperature_measured is false. Use integer arithmetic and no code fence.')
        prompts = {
            "E": (record_prefix + "\n\n" + extract + SUFFIX, {"facts": wanted}),
            "C": (original.split("<|im_start|>user\n", 1)[0] + "<|im_start|>user\n" +
                  "\n".join(selected) + "\n\n" + calc + SUFFIX, oracle),
            "T": (record_prefix + "\n\n" + trace + SUFFIX,
                  {"facts": wanted, "rows": oracle["rows"], "temperature_measured": False}),
        }
        for kind, (prompt, expected) in prompts.items():
            name = f"{case}-{kind}"
            prompt_data = prompt.encode("utf-8")
            expected_data = json_bytes(expected)
            put(output / f"{name}.prompt.txt", prompt_data)
            put(output / f"{name}.oracle.json", expected_data)
            probes.append({"id": name, "case": case, "kind": kind,
                           "prompt_sha256": digest(prompt_data),
                           "oracle_sha256": digest(expected_data),
                           "source_prompt_sha256": digest(original_file.read_bytes()),
                           "prior_response_sha256": digest(prior_file.read_bytes()),
                           "tokens": None, "token_ids_sha256": None})
    manifest = {"schema": "p4.release-a.i0-answer-cause.v1", "settings": SETTING,
                "order": [row["id"] for row in probes], "probes": probes}
    put(output / "manifest.json", json_bytes(manifest))
    return manifest


def tokenize(inputs: Path, tokenizer: Path, model: Path, expected_sha256: str) -> dict:
    if digest(tokenizer.read_bytes()) != expected_sha256:
        raise ValueError("tokenizer binary identity differs")
    manifest_file = inputs / "manifest.json"
    manifest = json.loads(manifest_file.read_text(encoding="utf-8"))
    if manifest.get("schema") != "p4.release-a.i0-answer-cause.v1" or len(manifest.get("probes", [])) != 6:
        raise ValueError("diagnostic manifest differs")
    if any(row["tokens"] is not None or row["token_ids_sha256"] is not None for row in manifest["probes"]):
        raise ValueError("probe tokens were already sealed")
    environment = {**os.environ, "PATH": str(tokenizer.parent) + os.pathsep + os.environ.get("PATH", "")}
    with (inputs / "tokenizer.stderr.log").open("xb") as stderr:
        process = subprocess.Popen([str(tokenizer), str(model)], stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=stderr, text=True,
                                   encoding="utf-8", env=environment)
        try:
            assert process.stdin is not None and process.stdout is not None
            for row in manifest["probes"]:
                name = row["id"]
                prompt_file = inputs / f"{name}.prompt.txt"
                if digest(prompt_file.read_bytes()) != row["prompt_sha256"]:
                    raise ValueError(f"prompt differs before tokenization: {name}")
                token_file = inputs / f"{name}.tokens.bin"
                if token_file.exists():
                    raise FileExistsError(token_file)
                process.stdin.write(f"{prompt_file.resolve()}\t{token_file.resolve()}\n")
                process.stdin.flush()
                line = process.stdout.readline().strip()
                if not re.fullmatch(r"[1-9][0-9]*", line):
                    raise ValueError(f"tokenizer count differs: {name}: {line!r}")
                token_data = token_file.read_bytes()
                count = int(line)
                if len(token_data) != count * struct.calcsize("<i"):
                    raise ValueError(f"tokenizer ID extent differs: {name}")
                row.update(tokens=count, token_ids_sha256=digest(token_data))
            process.stdin.close()
            if process.wait(timeout=120) != 0:
                raise RuntimeError(f"tokenizer exited {process.returncode}")
        except BaseException:
            process.kill()
            process.wait()
            raise
    manifest["tokenizer_sha256"] = expected_sha256
    manifest["model_path"] = str(model)
    manifest_file.write_bytes(json_bytes(manifest))
    return manifest


def evaluate(inputs: Path, responses: Path) -> dict:
    manifest = json.loads((inputs / "manifest.json").read_text(encoding="utf-8"))
    if manifest.get("schema") != "p4.release-a.i0-answer-cause.v1" or len(manifest.get("probes", [])) != 6:
        raise ValueError("diagnostic manifest differs")
    rows = []
    for row in manifest["probes"]:
        name = row["id"]
        failures = []
        try:
            prompt_bytes = (inputs / f"{name}.prompt.txt").read_bytes()
            oracle_bytes = (inputs / f"{name}.oracle.json").read_bytes()
            token_bytes = (inputs / f"{name}.tokens.bin").read_bytes()
            response = json.loads((responses / f"{name}.response.json").read_text(encoding="utf-8"))
            if not isinstance(response, dict):
                raise ValueError("invalid response shape")
            expected = json.loads(oracle_bytes)
            prompt = prompt_bytes.decode("utf-8")
        except (OSError, UnicodeError, ValueError, TypeError) as error:
            rows.append({"id": name, "passed": False, "failures": [f"evidence_invalid:{type(error).__name__}"]})
            continue
        if digest(prompt_bytes) != row["prompt_sha256"] or digest(oracle_bytes) != row["oracle_sha256"]:
            failures.append("input_digest_mismatch")
        if not token_bytes or len(token_bytes) % struct.calcsize("<i") or row.get("tokens") != len(token_bytes) // 4 or digest(token_bytes) != row.get("token_ids_sha256"):
            failures.append("token_binding_mismatch")
        if response.get("prompt") != prompt:
            failures.append("prompt_mismatch")
        if type(response.get("tokens_evaluated")) is not int or response["tokens_evaluated"] != len(token_bytes) // 4:
            failures.append("token_count_mismatch")
        if response.get("stop") is not True or response.get("stop_type") != "eos" or response.get("truncated") is not False:
            failures.append("non_eos")
        settings = response.get("generation_settings")
        if not isinstance(settings, dict) or any(settings.get(key) != value for key, value in SETTING.items()):
            failures.append("settings_mismatch")
        try:
            actual = json.loads(response["content"])
        except (KeyError, TypeError, ValueError):
            actual = None
            failures.append("invalid_json")
        if isinstance(actual, dict):
            for key, value in expected.items():
                if actual.get(key) != value:
                    failures.append(f"{key}_mismatch")
            if set(actual) != set(expected):
                failures.append("schema_mismatch")
        elif "invalid_json" not in failures:
            failures.append("schema_mismatch")
        rows.append({"id": name, "passed": not failures, "failures": failures,
                     "tokens_evaluated": response.get("tokens_evaluated")})
    return {"passed": all(row["passed"] for row in rows), "probes": rows}


def self_test() -> None:
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        corpus, prior, inputs, responses = (root / name for name in ("corpus", "prior", "inputs", "responses"))
        corpus.mkdir(); prior.mkdir(); responses.mkdir()
        for case in CASES:
            facts = [{"id": "R00002", "revision": 1, "amps": 2, "milliohms": 3, "hours": 4, "pressure": 121}]
            # Three distinct records exercise the exact source-ID transcriber.
            facts += [{**facts[0], "id": "R00003"}, {**facts[0], "id": "R00004"}]
            source = "\n".join(f"[{f['id']}] revision {f['revision']}. Measured RMS current: {f['amps']} A. Isolated conductor resistance: {f['milliohms']} milliohms. Operating duration: {f['hours']} hours. Inlet pressure: {f['pressure']} kPa." for f in facts)
            prompt = "<|im_start|>system\nsource<|im_end|>\n<|im_start|>user\n" + source + "\n\nConnect the source facts for R00002" + SUFFIX
            (corpus / f"{case}.prompt.txt").write_text(prompt, encoding="utf-8")
            (corpus / f"{case}.facts.json").write_bytes(json_bytes(facts))
            oracle = {"rows": [{"id": f["id"], "revision": 1, "power_mW": 12, "energy_mWh": 48,
                                "pressure_alarm": True} for f in facts], "temperature_measured": False}
            (corpus / f"{case}.oracle.json").write_bytes(json_bytes(oracle))
            (prior / f"{case}.response.json").write_bytes(json_bytes({"prompt": prompt}))
        manifest = prepare(corpus, prior, inputs)
        for row in manifest["probes"]:
            name = row["id"]
            tokens = struct.pack("<ii", 1, 2)
            (inputs / f"{name}.tokens.bin").write_bytes(tokens)
            row.update(tokens=2, token_ids_sha256=digest(tokens))
            expected = json.loads((inputs / f"{name}.oracle.json").read_text(encoding="utf-8"))
            response = {"prompt": (inputs / f"{name}.prompt.txt").read_text(encoding="utf-8"),
                        "tokens_evaluated": 2, "stop": True, "stop_type": "eos", "truncated": False,
                        "generation_settings": SETTING, "content": json.dumps(expected)}
            (responses / f"{name}.response.json").write_bytes(json_bytes(response))
        (inputs / "manifest.json").write_bytes(json_bytes(manifest))
        assert evaluate(inputs, responses)["passed"]
        target = responses / "case-04-T.response.json"
        baseline = json.loads(target.read_text(encoding="utf-8"))
        wrong = copy.deepcopy(baseline)
        actual = json.loads(wrong["content"])
        actual["facts"][0]["amps"] = 9
        wrong["content"] = json.dumps(actual)
        target.write_bytes(json_bytes(wrong))
        assert "facts_mismatch" in evaluate(inputs, responses)["probes"][2]["failures"]
        wrong = copy.deepcopy(baseline)
        actual = json.loads(wrong["content"])
        actual["rows"][0]["power_mW"] = 13
        wrong["content"] = json.dumps(actual)
        target.write_bytes(json_bytes(wrong))
        assert "rows_mismatch" in evaluate(inputs, responses)["probes"][2]["failures"]
        for change, code in (({"prompt": "swapped"}, "prompt_mismatch"),
                             ({"tokens_evaluated": 1}, "token_count_mismatch"),
                             ({"stop_type": "length"}, "non_eos"),
                             ({"content": "```json\n{}\n```"}, "invalid_json")):
            wrong = copy.deepcopy(baseline); wrong.update(change)
            target.write_bytes(json_bytes(wrong))
            assert code in evaluate(inputs, responses)["probes"][2]["failures"]
        target.unlink()
        assert evaluate(inputs, responses)["probes"][2]["failures"] == ["evidence_invalid:FileNotFoundError"]
        print(json.dumps({"passed": True, "mutations": 7}, separators=(",", ":")))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("prepare", "tokenize", "judge", "self-test"))
    parser.add_argument("--corpus", type=Path)
    parser.add_argument("--prior", type=Path)
    parser.add_argument("--inputs", type=Path)
    parser.add_argument("--responses", type=Path)
    parser.add_argument("--tokenizer", type=Path)
    parser.add_argument("--model", type=Path)
    parser.add_argument("--tokenizer-sha256")
    args = parser.parse_args()
    if args.mode == "self-test":
        self_test()
    elif args.mode == "prepare":
        if not args.corpus or not args.prior or not args.inputs:
            parser.error("prepare requires --corpus, --prior, --inputs")
        print(json.dumps(prepare(args.corpus, args.prior, args.inputs), separators=(",", ":")))
    elif args.mode == "tokenize":
        if not args.inputs or not args.tokenizer or not args.model or not args.tokenizer_sha256:
            parser.error("tokenize requires --inputs, --tokenizer, --model, --tokenizer-sha256")
        result = tokenize(args.inputs, args.tokenizer, args.model, args.tokenizer_sha256)
        print(json.dumps({"passed": True, "probes": len(result["probes"]),
                          "tokens": {row["id"]: row["tokens"] for row in result["probes"]}},
                         separators=(",", ":")))
    else:
        if not args.inputs or not args.responses:
            parser.error("judge requires --inputs and --responses")
        result = evaluate(args.inputs, args.responses)
        print(json.dumps(result, separators=(",", ":")))
        if not result["passed"]:
            raise SystemExit(1)


if __name__ == "__main__":
    main()
