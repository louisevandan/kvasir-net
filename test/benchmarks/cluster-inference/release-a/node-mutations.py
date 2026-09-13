"""Run baseline and removed-guard tests in independent source copies."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parent
CASES = {
    "baseline": None,
    "receipt_product": ("manifest.mjs", "fail(count * each <= bytes, 'response ownership under-reserved');", "", 1),
    "shared_pool": ("manifest.mjs", "fail(pool.reserved <= pool.available, 'shared pool overcommitted');", "", 2),
    "exact_oracle": ("corpus.mjs", "assert.deepEqual(value, item.expected);", "", 1),
    "missing_as_zero": ("analyze-trace.mjs", "percentile(s.spans.map(x => x.rpc), .5) : null", "percentile(s.spans.map(x => x.rpc), .5) : 0", 1),
}


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main(output):
    output.mkdir(parents=True, exist_ok=False)
    sources = {p.name: sha(p) for p in ROOT.glob("*.mjs")}
    records = []
    for name, change in CASES.items():
        case = output / name; case.mkdir()
        for source in ROOT.glob("*.mjs"):
            shutil.copyfile(source, case / source.name)
        mutation = None
        if change:
            p = case / change[0]; value = p.read_text(encoding="utf-8")
            assert value.count(change[1]) == change[3], name
            before = sha(p); p.write_text(value.replace(change[1], change[2]), encoding="utf-8", newline="\n")
            mutation = {"path": change[0], "before": before, "after": sha(p)}
        command = ["node", "--test", *[p.name for p in sorted(case.glob("*.test.mjs"))]]
        with (case / "test.log").open("wb") as log:
            result = subprocess.run(command, cwd=case, stdout=log, stderr=subprocess.STDOUT, timeout=60)
        text = (case / "test.log").read_text(encoding="utf-8", errors="replace")
        ok = result.returncode == (0 if change is None else 1)
        if change: ok = ok and "ERR_ASSERTION" in text
        records.append({"case": name, "ok": ok, "exit": result.returncode, "command": command,
                        "mutation": mutation, "log_sha256": sha(case / "test.log")})
        (output / "summary.json").write_text(json.dumps({"sources": sources, "cases": records}, indent=2), encoding="utf-8")
        print(name, "PASS" if ok else "FAIL", flush=True)
    return 0 if all(r["ok"] for r in records) else 1


if __name__ == "__main__":
    p = argparse.ArgumentParser(); p.add_argument("output", type=Path)
    raise SystemExit(main(p.parse_args().output.resolve()))
