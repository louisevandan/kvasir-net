"""Failure and recovery through actual Agent-target NODE_LOAD and NODE_UNLOAD."""

import argparse
import hashlib
import json
from pathlib import Path
import sys


ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "python"))

from p4hfadapter.integration.lifecycle import (  # noqa: E402
    ADAPTER_KIND,
    NODE_LIFECYCLE_RESULT,
    NODE_LOAD,
    NODE_UNLOAD,
    decode_result,
    encode_request,
)
from p4hfadapter.integration.packet import pack, unpack  # noqa: E402
from p4hfadapter.integration.transport import Client  # noqa: E402


COMMAND = "application/vnd.p4.hf.command-v2"
RESULT = "application/vnd.p4.hf.result-v2"
CAPACITIES = {
    "queue_capacity": 1,
    "completion_capacity": 1,
    "retained_capacity": 2,
    "retained_bytes": 32768,
}


def main(args):
    args.output.mkdir(parents=True, exist_ok=False)
    client = Client(args.host, args.port)
    results = []

    for mode in (
        "normal",
        "ready_mismatch",
        "death",
        "partial",
        "magic",
        "version",
        "reserved",
        "length",
        "identity",
        "hang",
        "incompatible",
        "hash_mismatch",
    ):
        node = {
            "agent": client.address,
            "node": f"fixture-{client.outer[2][:8]}-{mode}",
            "generation": 1,
        }
        folder = args.output / mode
        folder.mkdir()
        worker = (ROOT / "tests/fixtures/bridge_worker/main.py").read_bytes()
        (folder / "entry.py").write_bytes(worker)
        manifest = {
            "protocol": 3 if mode == "incompatible" else 2,
            "entry": "entry.py",
            "files": {"entry.py": hashlib.sha256(worker).hexdigest()},
        }
        bundle = folder / "bundle.json"
        bundle.write_text(json.dumps(manifest), encoding="utf-8")
        topology = [node]

        def command(meta):
            _, body = client.exchange(
                (1, node["agent"], node["node"], node["generation"]),
                COMMAND,
                pack(meta),
                ADAPTER_KIND,
            )
            return unpack(body)[0]

        def lifecycle(operation, meta):
            content = NODE_LOAD if operation == "load" else NODE_UNLOAD
            envelope, body = client.exchange(
                (0, node["agent"]),
                content,
                encode_request(
                    operation,
                    node,
                    COMMAND,
                    pack(meta),
                    CAPACITIES if operation == "load" else None,
                ),
                ADAPTER_KIND,
            )
            if (
                envelope["source"] != (0, node["agent"])
                or envelope["content"] != NODE_LIFECYCLE_RESULT
            ):
                raise ValueError("node lifecycle response envelope mismatch")
            state, opaque = decode_result(body, operation, node, RESULT)
            reply = unpack(opaque)[0] if opaque else {}
            return state, reply

        def job(kind, serial=1, epoch=1):
            return {
                "job": {
                    "generation": 1,
                    "epoch": epoch,
                    "serial": serial,
                    "kind": kind,
                    "request": "A",
                    "issue": 0,
                    "position": 0,
                },
                "receipts": [],
            }

        record = {"mode": mode, "replies": [], "lifecycle": []}
        results.append(record)
        owned = False
        try:
            launch = {
                "python": args.python,
                "bundle": str(bundle.resolve()),
                "bundle_sha256": (
                    "00" * 32
                    if mode == "hash_mismatch"
                    else hashlib.sha256(bundle.read_bytes()).hexdigest()
                ),
                "config": {"mode": mode},
                "identity": {"generation": 1, "index": 0, "nodes": topology},
                "frame_bytes": 2048,
                "scratch_bytes": 8192,
                "stderr_bytes": 1024,
                "timeout_ms": 500,
            }
            owned = True
            state, reply = lifecycle(
                "load",
                {
                    "op": "load",
                    "generation": 1,
                    "nodes": topology,
                    "index": 0,
                    "launch": launch,
                },
            )
            record["lifecycle"].append(state)
            record["replies"].append(reply)
            if state["resource_state"] == "absent":
                owned = False
            if mode in ("ready_mismatch", "incompatible", "hash_mismatch"):
                assert state["status"] != "succeeded"
                assert state["resource_state"] == "absent"
            else:
                assert state["status"] == "succeeded"
                assert state["resource_state"] == "present"
                assert reply["ok"]
                reply = command(job("step"))
                record["replies"].append(reply)
                if mode == "normal":
                    assert reply["ok"]
                    state, reply = lifecycle("unload", job("unload", 2))
                    record["lifecycle"].append(state)
                    record["replies"].append(reply)
                    assert state["status"] == "rejected"
                    assert state["resource_state"] == "present"
                    assert command(job("cancel", 2))["ok"]
                    assert command(job("epoch", 3, 2))["ok"]
                    assert command(job("release", 4, 1))["ok"] is False
                    assert command(job("step", 4, 2))["ok"]
                    assert command(job("release", 5, 2))["ok"]
                    state, reply = lifecycle("unload", job("unload", 6, 2))
                    record["lifecycle"].append(state)
                    record["replies"].append(reply)
                    assert state["status"] == "succeeded"
                    assert state["resource_state"] == "absent"
                    assert reply["ok"]
                    owned = False
                else:
                    assert reply["ok"] is False and reply["uncertain"]
                    assert command(job("step", 2))["ok"] is False
            record["ok"] = True
        except Exception as error:
            record["ok"] = False
            record["first_error"] = str(error)
        finally:
            if owned:
                abort = job("abort", 0, 0)
                abort["job"]["request"] = ""
                try:
                    state, reply = lifecycle("unload", abort)
                    record["lifecycle"].append(state)
                    record["replies"].append(reply)
                    assert state["resource_state"] == "absent"
                    owned = False
                except Exception as error:
                    record["cleanup_error"] = str(error)
                    record["ok"] = False
            (args.output / "summary.json").write_text(
                json.dumps({"cases": results, "trace": client.trace}, indent=2) + "\n",
                encoding="utf-8",
            )
        print(json.dumps(record), flush=True)
    client.close()
    return 0 if all(result["ok"] for result in results) else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", default="192.168.0.6")
    parser.add_argument("--port", type=int, default=41980)
    parser.add_argument("--python", default=sys.executable)
    parser.add_argument("--output", type=Path, required=True)
    raise SystemExit(main(parser.parse_args()))
