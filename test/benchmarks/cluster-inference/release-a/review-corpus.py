"""Check exact source facts independently of the JS generator and preserve legacy inputs."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main(args):
    args.output.mkdir(parents=True, exist_ok=False)
    manifest = json.loads((args.corpus / "corpus.json").read_text(encoding="utf-8"))
    checks = []
    for item in manifest["requests"]:
        stem = args.corpus / item["id"]
        text = stem.with_suffix(".prompt.txt").read_text(encoding="utf-8")
        expected = json.loads(stem.with_suffix(".oracle.json").read_text(encoding="utf-8"))
        facts = {m[0]: tuple(map(int, m[1:])) for m in re.findall(
            r"\[(R\d+)\] Station (\d+); revision (\d+)\. Measured RMS current: (\d+) A\. "
            r"Isolated conductor resistance: (\d+) milliohms\. Operating duration: (\d+) hours\. "
            r"Inlet pressure: (\d+) kPa\.", text)}
        assert len(facts) == item["records"]
        if item["class"] == "short":
            assert expected["replacement"] == "return pressure > threshold;"
            assert expected["checks"][-1] == {"id": "boundary", "alarm": False}
            for row in expected["checks"][:-1]:
                assert row["alarm"] == (facts[row["id"]][5] > 120)
        else:
            assert expected["temperature_measured"] is False
            for row in expected["rows"]:
                _, revision, amps, resistance, hours, pressure = facts[row["id"]]
                assert row == dict(id=row["id"], revision=revision, power_mW=amps*amps*resistance,
                                  energy_mWh=amps*amps*resistance*hours, pressure_alarm=pressure > 120)
        assert sha(stem.with_suffix(".prompt.txt")) == item["prompt_sha256"]
        assert sha(stem.with_suffix(".tokens.bin")) == item["token_ids_sha256"]
        assert sha(stem.with_suffix(".oracle.json")) == item["oracle_sha256"]
        assert stem.with_suffix(".tokens.bin").stat().st_size == item["input_tokens"] * 4
        checks.append({"case": item["id"], "facts": len(facts), "ok": True})
    (args.output / "source-facts.json").write_text(json.dumps(checks, indent=2), encoding="utf-8")
    print(f"SOURCE_FACT_ORACLES {len(checks)} PASS", flush=True)
    if args.legacy:
        old = json.loads((args.legacy / "workload.json").read_text(encoding="utf-8"))
        files = [args.legacy / f"long-final-{i}.txt" for i in range(8)]
        assert [sha(p) for p in files] == old["prompt_sha256"]
        command = [str(args.tokenizer.resolve()), args.model]
        result = subprocess.run(command, input=("\n".join(str(p.resolve()) for p in files) + "\n").encode("utf-8"),
            capture_output=True, timeout=60,
            env=dict(os.environ, PATH=str(args.runtime.resolve()) + os.pathsep + os.environ["PATH"]))
        (args.output / "legacy-tokenizer.stderr.log").write_bytes(result.stderr)
        assert result.returncode == 0, result.stderr
        counts = list(map(int, result.stdout.splitlines()))
        assert counts == old["token_counts"], counts
        (args.output / "legacy.json").write_text(json.dumps({"counts": counts, "command": command,
            "original_prompt_sha256": old["prompt_sha256"], "exit": result.returncode}, indent=2), encoding="utf-8")
        print(f"LEGACY_INPUTS {counts} PASS")


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("corpus", type=Path); p.add_argument("output", type=Path)
    p.add_argument("--legacy", type=Path); p.add_argument("--tokenizer", type=Path)
    p.add_argument("--runtime", type=Path); p.add_argument("--model")
    main(p.parse_args())
