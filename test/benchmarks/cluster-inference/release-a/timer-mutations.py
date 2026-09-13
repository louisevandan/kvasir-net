"""Recompile the actual worker tests in isolated copies; never mutate the checkout."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import zipfile

ROOT = Path(__file__).resolve().parents[4]
WORKER = "layers/adapters/llamacpp/staged/adapter/src/v2/node/worker"
HELPER = WORKER + "/loop_tests/bounded_strategy/phase_pacing.rs"
OVERLAYS = [WORKER + "/loop_tests/bounded_strategy.rs", HELPER]
PREFIX = "v2::node::worker::loop_tests::bounded_strategy::phase_pacing_actual_loop_"
CASES = {
    "baseline": None,
    "old_global_freeze": (HELPER, "seen.before = Some(expected);", "// Removed RELEASE linearization.",
                          "preserves_authority_across_a_real_release_during_wait",
                          "timer wait must preserve request/flight/slot/input authority"),
    "timer_grants_event": (WORKER + ".rs", "Err(mpsc::RecvTimeoutError::Timeout) => None,",
                           "Err(mpsc::RecvTimeoutError::Timeout) => { self.state.next_event += 1; None },",
                           "expires_decode_wait_without_another_tail_or_input",
                           "timer wait must preserve request/flight/slot/input authority"),
}


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main(output):
    output.mkdir(parents=True, exist_ok=False)
    archives = {}
    sources = {}
    for name, root in (("p4", ROOT), ("p4hfadapter", ROOT.parent / "p4hfadapter")):
        archive = output / (name + ".zip")
        subprocess.run(["git", "archive", "--format=zip", f"--output={archive}", "HEAD"], cwd=root, check=True)
        archives[name] = archive
        sources[name] = {"head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
                         "archive_sha256": sha(archive)}
    overlay_bytes = {rel: (ROOT / rel).read_bytes() for rel in OVERLAYS}
    sources["overlays"] = {rel: hashlib.sha256(data).hexdigest() for rel, data in overlay_bytes.items()}
    (output / "sources.json").write_text(json.dumps(sources, indent=2), encoding="utf-8")
    records = []
    for name, change in CASES.items():
        case = output / name
        for repo, archive in archives.items():
            with zipfile.ZipFile(archive) as z:
                z.extractall(case / repo)
        source = case / "p4"
        for rel, data in overlay_bytes.items():
            dest = source / rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes(data)
        mutation = None
        if change:
            path = source / change[0]
            text = path.read_text(encoding="utf-8")
            if text.count(change[1]) != 1:
                raise RuntimeError(f"mutation drift: {name}")
            before = sha(path)
            path.write_text(text.replace(change[1], change[2]), encoding="utf-8", newline="\n")
            mutation = {"path": change[0], "before": before, "after": sha(path)}
        env = dict(os.environ, CARGO_TARGET_DIR=str(case / "target"))
        cmd = ["cargo", "test", "--locked", "-p", "p4-llamacpp-staged-adapter", "--lib",
               PREFIX + change[3] if change else PREFIX, "--", "--nocapture"]
        if change:
            cmd.append("--exact")
        print(f"START {name}", flush=True)
        with (case / "test.log").open("wb") as log:
            result = subprocess.run(cmd, cwd=source, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=600)
        log = (case / "test.log").read_text(encoding="utf-8", errors="replace")
        binaries = list((case / "target/debug/deps").glob("p4_llamacpp_staged_adapter-*.exe"))
        ok = (result.returncode == 0 and "3 passed; 0 failed" in log) if not change else (
            result.returncode == 101 and "test result: FAILED" in log and change[4] in log)
        ok = ok and "Compiling p4-llamacpp-staged-adapter" in log and len(binaries) == 1
        record = {"case": name, "ok": ok, "exit": result.returncode, "command": cmd,
                  "mutation": mutation, "binary_sha256": sha(binaries[0]) if binaries else None,
                  "target": env["CARGO_TARGET_DIR"], "log_sha256": sha(case / "test.log")}
        records.append(record)
        (output / "summary.json").write_text(json.dumps(records, indent=2), encoding="utf-8")
        print(json.dumps(record), flush=True)
    return 0 if all(r["ok"] for r in records) else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("output", type=Path)
    raise SystemExit(main(parser.parse_args().output.resolve()))
