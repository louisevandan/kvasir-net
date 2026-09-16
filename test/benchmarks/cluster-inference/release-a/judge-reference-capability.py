#!/usr/bin/env python3
"""Gate exact I0 service capability from standalone model output and source facts."""

from __future__ import annotations

import argparse
import importlib.util
import json
import struct
import tempfile
from pathlib import Path


CASES = ("case-00", "case-04", "case-06")


def source_verifier():
    path = Path(__file__).with_name("judge-h1-quality.py")
    spec = importlib.util.spec_from_file_location("release_a_h1_judge", path)
    if spec is None or spec.loader is None:
        raise RuntimeError("source verifier unavailable")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.verified_source_response


def evaluate(corpus: Path, responses: Path) -> dict:
    rows = []
    verify_source = source_verifier()
    for case in CASES:
        failures = []
        try:
            prompt = (corpus / f"{case}.prompt.txt").read_text(encoding="utf-8")
            oracle = json.loads((corpus / f"{case}.oracle.json").read_text(encoding="utf-8"))
            token_bytes = (corpus / f"{case}.tokens.bin").read_bytes()
            if len(token_bytes) % struct.calcsize("<i") or not token_bytes:
                failures.append("invalid_token_file")
            response = json.loads((responses / f"{case}.response.json").read_text(encoding="utf-8"))
        except (OSError, ValueError, TypeError) as error:
            rows.append({"case": case, "passed": False, "failures": [f"evidence_missing_or_invalid:{type(error).__name__}"]})
            continue
        if not isinstance(response, dict):
            rows.append({"case": case, "passed": False, "failures": ["invalid_response_shape"]})
            continue
        if response.get("prompt") != prompt:
            failures.append("prompt_mismatch")
        if type(response.get("tokens_evaluated")) is not int or response["tokens_evaluated"] != len(token_bytes) // 4:
            failures.append("token_count_mismatch")
        if response.get("stop") is not True or response.get("stop_type") != "eos" or response.get("truncated") is not False:
            failures.append("non_eos")
        predicted = response.get("tokens_predicted")
        if type(predicted) is not int or not 0 < predicted <= 2048:
            failures.append("output_token_count")
        raw = response.get("content")
        if case == "case-00":
            try:
                actual = json.loads(raw)
            except (TypeError, ValueError):
                failures.append("invalid_json")
            else:
                if actual != oracle:
                    failures.append("oracle_mismatch")
        elif not isinstance(raw, str):
            failures.append("invalid_model_response")
        else:
            # The oracle is only the expected result being checked. The
            # verifier independently derives every number from the prompt;
            # it also requires the model's raw IDs, order and revisions.
            failures.extend(verify_source({
                "prompt": prompt,
                "model_response": raw,
                "outcomes": [{"text": raw}],
                "response_processor": "engineering_power_v1",
                "service_error": None,
                "response": json.dumps(oracle),
            }, check_timing=False))
        rows.append({"case": case, "passed": not failures, "failures": failures})
    return {"passed": all(row["passed"] for row in rows), "cases": rows}


def self_test() -> None:
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        corpus, responses = root / "corpus", root / "responses"
        corpus.mkdir()
        responses.mkdir()
        baseline = {"prompt": "example", "tokens_evaluated": 2, "stop": True,
                    "stop_type": "eos", "truncated": False, "tokens_predicted": 1,
                    "content": '{"answer": 4}'}
        source_row = ("[R00002] Station 7; revision 6. Measured RMS current: 28 A. "
                      "Isolated conductor resistance: 52 milliohms. Operating duration: 10 hours. "
                      "Inlet pressure: 121 kPa. Pressure alarm threshold: 120 kPa; "
                      "equality is not an exceedance. Temperature was not measured.")
        power_task = ("Compute power in milliwatts as current_A squared times resistance_milliohms, "
                      "and energy in milliwatt-hours as power_mW times duration_hours. "
                      'Return JSON with keys "rows" and "temperature_measured". Each row must contain '
                      '"id", "revision", "power_mW", "energy_mWh", and boolean "pressure_alarm". '
                      '"temperature_measured" must state whether those records contain a measured temperature. '
                      "Use integer arithmetic; do not infer a temperature or a pressure/heat causal relation.")
        source_prompt = ("<|im_start|>user\nCase 5: 1 archived records.\n"
                         f"{source_row}\n\nConnect the source facts for R00002, in that order. "
                         f"{power_task}<|im_end|>")
        source_oracle = {"rows": [{"id": "R00002", "revision": 6, "power_mW": 40768,
                                   "energy_mWh": 407680, "pressure_alarm": True}],
                         "temperature_measured": False}
        model_raw = json.dumps({"rows": [{"id": "R00002", "revision": 6,
                                           "power_mW": 1, "energy_mWh": 2,
                                           "pressure_alarm": False}],
                                "temperature_measured": False})
        for case in CASES:
            grounded = case != "case-00"
            case_prompt = source_prompt if grounded else "example"
            oracle = source_oracle if grounded else {"answer": 4}
            (corpus / f"{case}.prompt.txt").write_text(case_prompt, encoding="utf-8")
            (corpus / f"{case}.oracle.json").write_text(json.dumps(oracle), encoding="utf-8")
            (corpus / f"{case}.tokens.bin").write_bytes(struct.pack("<ii", 1, 2))
            row = dict(baseline, prompt=case_prompt, content=model_raw if grounded else baseline["content"])
            (responses / f"{case}.response.json").write_text(json.dumps(row), encoding="utf-8")
        assert evaluate(corpus, responses)["passed"]
        changes = ({"prompt": "other"}, {"tokens_evaluated": 1}, {"stop_type": "length"},
                   {"content": "```json\n{}"},
                   {"content": model_raw.replace('"revision": 6', '"revision": 7')},
                   {"content": model_raw.replace('"id": "R00002"', '"id": "R00003"')})
        for change in changes:
            altered = json.loads((responses / "case-06.response.json").read_text(encoding="utf-8"))
            altered.update(change)
            file = responses / "case-04.response.json"
            file.write_text(json.dumps(altered), encoding="utf-8")
            assert not evaluate(corpus, responses)["passed"], change
        file.write_text((responses / "case-06.response.json").read_text(encoding="utf-8"), encoding="utf-8")
        (corpus / "case-04.oracle.json").write_text('{"rows":[],"temperature_measured":false}', encoding="utf-8")
        assert "service_calculation_mismatch" in evaluate(corpus, responses)["cases"][1]["failures"]
        file.write_text("[]", encoding="utf-8")
        assert evaluate(corpus, responses)["cases"][1]["failures"] == ["invalid_response_shape"]
        file.unlink()
        assert evaluate(corpus, responses)["cases"][1]["failures"] == ["evidence_missing_or_invalid:FileNotFoundError"]
        print(json.dumps({"passed": True, "mutations": len(changes) + 3}, separators=(",", ":")))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--corpus", type=Path)
    parser.add_argument("--responses", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return
    if args.corpus is None or args.responses is None:
        parser.error("--corpus and --responses are required")
    result = evaluate(args.corpus, args.responses)
    print(json.dumps(result, separators=(",", ":")))
    if not result["passed"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
