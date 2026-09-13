"""One fresh local agent per diagnostic probe; retain failure and cleanup separately."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import time


def sha(p):
    return hashlib.sha256(Path(p).read_bytes()).hexdigest()


def main(args):
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    config = json.loads(args.config.read_text(encoding="utf-8-sig"))
    address = config["ingress_agent"].removeprefix("tcp://")
    host, port = address.rsplit(":", 1)
    if host != "127.0.0.1" or any(n["agent"] != config["ingress_agent"] for n in config["nodes"]):
        raise ValueError("diagnostic helper requires one local agent")
    if args.capture_native_processes and (os.name != "nt" or config.get("pre_inference_hold_ms", 0) < 5000):
        raise ValueError("native process capture requires Windows and a declared hold of at least 5000ms")
    with socket.socket() as s:
        if s.connect_ex((host, int(port))) == 0:
            raise ValueError("probe port already owned; refusing to reuse another agent")
    binaries = {str(args.agent.resolve()), str(args.driver.resolve())}
    for n in config["nodes"]:
        binaries.add(n["binary"])
        binaries.update(str(p) for p in Path(n["binary"]).parent.glob("*.dll"))
    record = {"binaries": {p: sha(p) for p in sorted(binaries)}, "first_error": None,
              "cleanup_error": None, "runtime_acceptance": False, "cost_conformance": None}
    (out / "config.json").write_text(json.dumps(config, indent=2), encoding="utf-8")
    env = os.environ.copy(); env["P4_STAGED_LLAMA_INHERIT_STDERR"] = "1"
    creationflags = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
    proc = None
    try:
        with (out / "agent.stdout.log").open("wb") as stdout, (out / "agent.stderr.log").open("wb") as stderr:
            proc = subprocess.Popen([str(args.agent.resolve()), address], env=env, stdout=stdout,
                                    stderr=stderr, creationflags=creationflags)
            record["owned_pid"] = proc.pid
            deadline = time.monotonic() + 30
            while True:
                if proc.poll() is not None:
                    raise RuntimeError("probe agent exited before readiness")
                with socket.socket() as s:
                    if s.connect_ex((host, int(port))) == 0:
                        break
                if time.monotonic() > deadline:
                    raise TimeoutError("probe agent readiness")
                time.sleep(.1)
            with (out / "driver.log").open("wb") as log:
                driver = subprocess.Popen([str(args.driver.resolve()), str(out / "config.json"),
                                           str(out / "artifact.json")], stdout=log, stderr=subprocess.STDOUT,
                                          creationflags=creationflags)
                deadline = time.monotonic() + config["timeout_ms"] / 1000 + 120
                try:
                    while driver.poll() is None:
                        if (args.capture_native_processes and "native_processes" not in record and
                                b"P4_EVENT_GATE_LOADED" in (out / "driver.log").read_bytes()):
                            command = ("$p=@(Get-CimInstance Win32_Process -Filter 'ParentProcessId=" +
                                       str(proc.pid) + "' | Select-Object ProcessId,ParentProcessId,ExecutablePath,CommandLine,CreationDate); ConvertTo-Json -InputObject $p -Depth 3")
                            raw = subprocess.check_output(["powershell", "-NoProfile", "-NonInteractive", "-Command", command],
                                                          creationflags=creationflags, timeout=20)
                            (out / "native-processes.raw.json").write_bytes(raw)
                            processes = json.loads(raw)
                            mapped = []
                            for index, node in enumerate(config["nodes"]):
                                port = node["endpoint"].rsplit(":", 1)[1]
                                found = [p for p in processes if re.search(r"--port\s+" + port + r"(?:\s|$)", p["CommandLine"]) and
                                         Path(p["ExecutablePath"]).resolve() == Path(node["binary"]).resolve()]
                                if len(found) != 1:
                                    raise ValueError("native process identity is ambiguous")
                                mapped.append({"node": index, "pid": found[0]["ProcessId"],
                                               "binary_sha256": sha(node["binary"]), "observed": found[0]})
                            record["native_processes"] = mapped
                            (out / "native-processes.json").write_text(json.dumps(mapped, indent=2), encoding="utf-8")
                        if time.monotonic() > deadline:
                            raise TimeoutError("driver exceeded diagnostic deadline")
                        time.sleep(.1)
                finally:
                    if driver.poll() is None:
                        driver.kill(); driver.wait(timeout=30)
            record["driver_exit"] = driver.returncode
            if (out / "artifact.json").exists():
                artifact = json.loads((out / "artifact.json").read_text(encoding="utf-8"))
                record["runtime_acceptance"] = artifact.get("passed") is True
                record["first_error"] = artifact.get("error")
                record["cleanup_error"] = artifact.get("cleanup_error")
    except Exception as error:
        record["first_error"] = str(error)
    finally:
        if proc is not None and proc.poll() is None:
            # Only the child created above and its descendants belong to this probe.
            if os.name == "nt":
                stopped = subprocess.run(["taskkill", "/PID", str(proc.pid), "/T", "/F"],
                                         capture_output=True, creationflags=creationflags)
                if stopped.returncode:
                    record["cleanup_error"] = repr(stopped.stderr)
            else:
                proc.terminate()
            try:
                proc.wait(timeout=30)
            except subprocess.TimeoutExpired:
                record["cleanup_error"] = "owned agent did not exit"
        try:
            subprocess.run(["node", str(Path(__file__).with_name("analyze-native-cost.mjs")),
                            str(out / "agent.stderr.log"), str(out / "cost.json")], check=True)
            cost = json.loads((out / "cost.json").read_text(encoding="utf-8"))
            record["cost_conformance"] = cost["diagnostic_conformance"]
        except Exception as error:
            record["cost_error"] = str(error)
        record["ok"] = (record["runtime_acceptance"] and record["first_error"] is None and
                        record["cleanup_error"] is None and
                        record["cost_conformance"] is (not args.expect_incomplete))
        (out / "summary.json").write_text(json.dumps(record, indent=2), encoding="utf-8")
        print(json.dumps({k: v for k, v in record.items() if k != "binaries"}, indent=2))
    return 0 if record["ok"] else 1


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("--config", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--agent", type=Path, required=True)
    p.add_argument("--driver", type=Path, required=True)
    p.add_argument("--expect-incomplete", action="store_true")
    p.add_argument("--capture-native-processes", action="store_true")
    raise SystemExit(main(p.parse_args()))
