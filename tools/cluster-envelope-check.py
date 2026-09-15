"""Model-free P4 return-route acceptance against explicitly supplied agents.

Run from the repository: python tools/cluster-envelope-check.py CONFIG OUTPUT
CONFIG is a JSON list of {name, host, port, address}; address is the advertised
P4 address, which can differ from the TCP dial address when using SSH tunnels.
No deployment, model loading, or process termination is performed.
"""
import concurrent.futures
import itertools
import json
from pathlib import Path
import struct
import sys
import time
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "layers/adapters/hf/python"))
from p4hfadapter.integration.transport import Client, endpoint, text

INSPECT = "application/vnd.p4.agent.inspect-v1+json"
CONNECTION_SEQUENCE = itertools.count(1)


def connect(agent, channel=None, generation=1):
    client = Client(agent["host"], agent["port"], timeout=15)
    client.address = agent["address"]
    client.outer = (2, client.address, channel or uuid.uuid4().hex, generation)
    # The integration client derives event IDs from channel and sequence only.
    # Distinct ingress/generation owners sharing a channel still require globally
    # unique event IDs; reserve a disjoint sequence block for each connection.
    client.sequence = next(CONNECTION_SEQUENCE) * 1_000_000
    return client


def queries(ingress, targets, channel=None, generation=1, burst=False):
    client = connect(ingress, channel, generation)
    pending, rows = {}, []
    try:
        def send(target):
            event = client.send((0, target["address"]), INSPECT, b"{}")
            pending[event] = (target, time.monotonic())

        def receive():
            meta, body = client.receive()
            target, started = pending.pop(meta["correlation"])
            assert meta["source"] == (0, target["address"]), meta
            assert meta["causation"] == meta["correlation"], meta
            assert meta["content"] == "application/vnd.p4.agent.snapshot-v1+json", meta
            snapshot = json.loads(body)
            assert snapshot["protocol_version"] == 3, snapshot
            assert snapshot["nodes"] == [], snapshot["nodes"]
            assert snapshot["broker"]["state"] == "ok", snapshot["broker"]
            rows.append(dict(ingress=ingress["name"], target=target["name"],
                             elapsed_ms=round((time.monotonic()-started)*1000, 3),
                             envelope=meta, snapshot=snapshot))

        for target in targets:
            send(target)
            if not burst:
                receive()
        while pending:
            receive()
        client.finish()
        return dict(ok=True, rows=rows, finish_ack=True)
    except Exception as error:
        return dict(ok=False, rows=rows, error=repr(error), pending=list(pending))
    finally:
        client.close()


def malformed(ingress, mode):
    client = connect(ingress)
    try:
        eid = uuid.uuid4().hex
        env = struct.pack("<H", 3)+text(eid)+text(eid)+b"\0"
        env += endpoint(client.outer)+endpoint((0, client.address))
        route = (2, client.address, client.outer[2], client.outer[3]+1)
        env += b"\0" if mode == "absent" else b"\1"+endpoint(route)[1:]
        env += b"\0"+struct.pack("<Q", 1)+b"\0\0"+text(INSPECT)
        data = b"P4E3"+struct.pack("<II", len(env), 2)+env+b"{}"
        client.socket.sendall(struct.pack("<I", len(data))+data)
        try:
            result = client.socket.recv(1)
            assert result == b"", result
        except ConnectionResetError:
            pass
        # Timeout is a failure: rejection must be observable.
        return dict(ok=True, mode=mode, ingress=ingress["name"])
    except Exception as error:
        return dict(ok=False, mode=mode, ingress=ingress["name"], error=repr(error))
    finally:
        client.close()


def main():
    agents = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8-sig"))
    output = Path(sys.argv[2])
    report = dict(agents=agents, cases=[])

    def record(name, results):
        report["cases"].append(dict(name=name, results=results))
        output.write_text(json.dumps(report, indent=2), encoding="utf-8")
        print(name, sum(x["ok"] for x in results), "/", len(results), flush=True)

    with concurrent.futures.ThreadPoolExecutor(max_workers=14) as pool:
        record("direct", list(pool.map(lambda a: queries(a, [a]), agents)))
        for round_number in range(3):
            record(f"matrix-{round_number+1}", list(pool.map(lambda a: queries(a, agents), agents)))
        record("burst", list(pool.map(lambda a: queries(a, agents*3, burst=True), agents)))
        shared = uuid.uuid4().hex
        work = [(a, g) for a in agents for g in [1, 2]]
        record("same-channel-distinct-generation", list(pool.map(
            lambda ag: queries(ag[0], agents, shared, ag[1], burst=True), work)))
        record("reconnect-generation-3", list(pool.map(
            lambda a: queries(a, agents, shared, 3), agents)))
        for mode in ["absent", "mismatch"]:
            record("reject-"+mode, list(pool.map(lambda a: malformed(a, mode), agents)))
        record("post-rejection", list(pool.map(lambda a: queries(a, [a]), agents)))
    report["passed"] = all(r["ok"] for c in report["cases"] for r in c["results"])
    report["responses"] = sum(len(r.get("rows", [])) for c in report["cases"] for r in c["results"])
    output.write_text(json.dumps(report, indent=2), encoding="utf-8")
    print("passed", report["passed"], "responses", report["responses"], flush=True)
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
