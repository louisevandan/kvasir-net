"""Prove guard sensitivity in independent, bytecode-free source copies."""

from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
from uuid import uuid4


def main() -> int:
    root = Path(__file__).resolve().parents[3]
    run_id = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-" + uuid4().hex[:8]
    output = root.parents[2] / "target/hf/framing" / run_id
    output.mkdir(parents=True)
    source_files = sorted(path for folder in ("python", "tests", "scripts/testing")
                          for path in (root / folder).rglob("*.py"))
    digest = {path.relative_to(root).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
              for path in source_files}
    decoder = "python/p4hfadapter/transport/framing/decoding/__init__.py"
    replacements = {
        "baseline": None,
        "version_guard_removed": (decoder, "if version != VERSION:", "if False:"),
        "size_guard_removed": (decoder, "if payload_size > limits.max_payload_bytes:", "if False:"),
        "receiver_fence_removed": ("python/p4hfadapter/transport/framing/receiving/__init__.py",
                                   "self._failed = True", "self._failed = False"),
        "sender_fence_removed": ("python/p4hfadapter/transport/framing/sending/__init__.py",
                                 "self._failed = True", "self._failed = False"),
        "closed_read_fix_removed": ("python/p4hfadapter/transport/framing/receiving/__init__.py",
                                    "except (OSError, ValueError) as error:", "except OSError as error:"),
    }
    results = []
    baseline_count = None
    for name, replacement in replacements.items():
        snapshot = output / name
        for path in source_files:
            target = snapshot / path.relative_to(root)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(path, target)
        mutation = None
        if replacement is not None:
            relative, old, new = replacement
            target = snapshot / relative
            text = target.read_text(encoding="utf-8")
            if text.count(old) != 1:
                raise RuntimeError(f"mutation target drift: {relative}")
            target.write_text(text.replace(old, new), encoding="utf-8", newline="\n")
            mutation = {"path": relative, "before": digest[relative],
                        "after": hashlib.sha256(target.read_bytes()).hexdigest()}
        env = dict(os.environ, PYTHONPATH=str(snapshot / "python"),
                   PYTHONDONTWRITEBYTECODE="1", PYTHONUTF8="1")
        command = [sys.executable, "-B", str(snapshot / "scripts/testing/run.py")]
        completed = subprocess.run(command, cwd=snapshot, env=env, capture_output=True, timeout=30)
        (snapshot / "tests.log").write_bytes(completed.stdout + completed.stderr)
        probe = subprocess.run([sys.executable, "-B", str(snapshot / "tests/fixtures/framing_peer/main.py")],
                               input=b"P4HF\x01" + b"\x00" * 11, cwd=snapshot, env=env,
                               capture_output=True, timeout=10)
        (snapshot / "peer.stderr.log").write_bytes(probe.stderr)
        if probe.returncode != 0 or probe.stdout != b"P4HF\x01" + b"\x00" * 11:
            raise RuntimeError("valid-frame peer probe failed")
        peer_identity = json.loads(probe.stderr.decode("utf-8").splitlines()[0])
        expected_decoder = snapshot / decoder
        if Path(peer_identity["decoder_source"]) != expected_decoder or peer_identity["decoder_sha256"] != hashlib.sha256(expected_decoder.read_bytes()).hexdigest():
            raise RuntimeError("peer used a source outside the snapshot")
        count_match = re.search(rb"Ran (\d+) tests?", completed.stderr)
        count = int(count_match[1]) if count_match else 0
        if replacement is None:
            baseline_count = count
        expected = (completed.returncode == 0 and count > 0) if replacement is None else (
            completed.returncode == 1 and count == baseline_count and count > 0
            and b"FAILED (" in completed.stderr)
        result = {"case": name, "command": command, "exit_code": completed.returncode,
                  "expected_result": expected, "test_count": count, "mutation": mutation, "peer_identity": peer_identity}
        results.append(result)
        print(json.dumps({"case": name, "exit_code": completed.returncode, "expected_result": expected}), flush=True)
    after = {path.relative_to(root).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
             for path in source_files}
    preserved = after == digest
    report = {"timestamp_utc": datetime.now(timezone.utc).isoformat(),
              "python": sys.version, "executable": sys.executable, "platform": platform.platform(),
              "mutation_runner_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              "source_sha256": digest, "original_source_preserved": preserved, "results": results}
    (output / "summary.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(output)
    return 0 if preserved and all(result["expected_result"] for result in results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
