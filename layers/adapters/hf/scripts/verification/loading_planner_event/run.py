"""Consume a generated loading plan through a task-owned local P4 agent."""
import argparse
import hashlib
import json
from pathlib import Path
import socket
import subprocess
import sys
import time

root = Path(__file__).resolve().parents[3]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--agent", type=Path, required=True)
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--scenario", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--model-dir", type=Path)
    args = parser.parse_args()
    output, agent = args.output.resolve(), args.agent.resolve()
    output.mkdir(parents=True, exist_ok=False)
    bundle = output / "bundle"
    subprocess.run([sys.executable, "-B", str(root / "scripts/deployment/worker/run.py"), str(bundle), "--label", "loading-planner"], check=True)
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    with (output / "agent.log").open("wb") as agent_log:
        process = subprocess.Popen([str(agent), f"127.0.0.1:{port}", f"tcp://127.0.0.1:{port}"], stdout=agent_log, stderr=subprocess.STDOUT,
                                   creationflags=subprocess.CREATE_NO_WINDOW if sys.platform == "win32" else 0)
        record = {"agent_pid": process.pid, "agent_sha256": hashlib.sha256(agent.read_bytes()).hexdigest(), "ok": False}
        try:
            for _ in range(100):
                if process.poll() is not None:
                    raise RuntimeError("owned agent exited before readiness")
                try:
                    with socket.create_connection(("127.0.0.1", port), timeout=.2):
                        break
                except OSError:
                    time.sleep(.1)
            else:
                raise TimeoutError("owned agent did not listen")
            command = [sys.executable, "-B", str(root / "scripts/verification/event_qwen/run.py"),
                       "--port", str(port), "--plan", str(args.plan.resolve()), "--scenario", str(args.scenario.resolve()),
                       "--bundle", str(bundle / "bundle.json"), "--output", str(output / "event"), "--agent-binary", str(agent)]
            if args.model_dir:
                command += ["--model-dir", str(args.model_dir.resolve())]
            run = subprocess.run(command, capture_output=True, timeout=600)
            (output / "event.log").write_bytes(run.stdout + run.stderr)
            record["command"] = command
            record["exit_code"] = run.returncode
            if run.returncode:
                raise AssertionError(f"event consumer failed: {output / 'event.log'}")
            result = json.loads((output / "event/summary.json").read_text(encoding="utf-8"))
            assert result["ok"] and result["comparisons"] and result["cache_comparisons"]
            assert result["first_error"] is None and result["cleanup_error"] is None
            record.update(ok=True, comparisons=len(result["comparisons"]), cache_comparisons=len(result["cache_comparisons"]))
        finally:
            if process.poll() is None:
                process.terminate()
            process.wait(timeout=30)
            record["agent_stopped"] = process.poll() is not None
            (output / "summary.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(record))


if __name__ == "__main__":
    main()
