"""Release A R9 physical-host hop receipt fault injection and acceptance.

The proxy drops exactly the first accepted hop receipt on its first connection.
The runner proves that the ingress agent retains the uncertain predecessor,
queries the receiver's exact receipt, acknowledges it, and only then releases
the queued request and a following normal wave.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import socket
import struct
import sys
import threading
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "layers/adapters/hf/python"))
from p4hfadapter.integration.transport import Client


INSPECT = "application/vnd.p4.agent.inspect-v1+json"
SNAPSHOT = "application/vnd.p4.agent.snapshot-v1+json"
RECONCILE = "application/vnd.p4.transport.reconcile-v1+json"
RESULT = "application/vnd.p4.node.result-v3+json"
HOP_MAGIC = b"P4H1"
HOP_RECEIPT = 4
MAX_FRAME = 2 * 1024 * 1024 * 1024


def _exact(stream: socket.socket, count: int) -> bytes:
    value = bytearray()
    while len(value) < count:
        block = stream.recv(count - len(value))
        if not block:
            raise EOFError("socket closed inside a framed message")
        value.extend(block)
    return bytes(value)


def _frame(stream: socket.socket) -> bytes:
    prefix = _exact(stream, 4)
    size, = struct.unpack("<I", prefix)
    if size > MAX_FRAME:
        raise ValueError(f"frame exceeds P4 transport bound: {size}")
    return prefix + (_exact(stream, size) if size else b"")


def _is_receipt(frame: bytes) -> bool:
    return len(frame) >= 13 and frame[4:8] == HOP_MAGIC and frame[12] == HOP_RECEIPT


class ReceiptDropProxy:
    def __init__(self, listen_host: str, listen_port: int, backend_host: str,
                 backend_port: int, output: Path):
        self.listen_host = listen_host
        self.listen_port = listen_port
        self.backend_host = backend_host
        self.backend_port = backend_port
        self.output = output
        self.lock = threading.Lock()
        self.dropped = False
        self.connections = 0
        self.events: list[dict[str, object]] = []

    def record(self, kind: str, **fields: object) -> None:
        with self.lock:
            self.events.append({"kind": kind, "unix_ms": int(time.time() * 1000), **fields})
            self.output.write_text(json.dumps({
                "listen": [self.listen_host, self.listen_port],
                "backend": [self.backend_host, self.backend_port],
                "connections": self.connections,
                "dropped": self.dropped,
                "events": self.events,
            }, indent=2), encoding="utf-8")

    def forward(self, source: socket.socket, target: socket.socket,
                drop_receipt: bool, stop: threading.Event) -> None:
        try:
            while not stop.is_set():
                frame = _frame(source)
                if drop_receipt and _is_receipt(frame):
                    with self.lock:
                        if not self.dropped:
                            self.dropped = True
                            digest = hashlib.sha256(frame).hexdigest()
                            exact = frame.hex()
                        else:
                            digest = exact = ""
                    if digest:
                        self.record("dropped_receipt", frame_bytes=len(frame),
                                    sha256=digest, exact_hex=exact)
                        stop.set()
                        for stream in (source, target):
                            try:
                                stream.shutdown(socket.SHUT_RDWR)
                            except OSError:
                                pass
                        return
                target.sendall(frame)
        except (EOFError, OSError, ValueError) as error:
            if not stop.is_set():
                self.record("direction_closed", detail=repr(error))
            stop.set()

    def handle(self, client: socket.socket) -> None:
        with client:
            backend = socket.create_connection((self.backend_host, self.backend_port), timeout=10)
            with backend:
                client.settimeout(30)
                backend.settimeout(30)
                with self.lock:
                    self.connections += 1
                    drop = not self.dropped
                    number = self.connections
                self.record("connection_opened", number=number, drop_receipt=drop)
                stop = threading.Event()
                upstream = threading.Thread(target=self.forward,
                    args=(client, backend, False, stop), daemon=True)
                downstream = threading.Thread(target=self.forward,
                    args=(backend, client, drop, stop), daemon=True)
                upstream.start(); downstream.start()
                upstream.join(); downstream.join()
                self.record("connection_closed", number=number)

    def serve(self) -> None:
        self.output.parent.mkdir(parents=True, exist_ok=True)
        with socket.create_server((self.listen_host, self.listen_port), reuse_port=False) as listener:
            listener.settimeout(1)
            self.record("ready")
            while True:
                try:
                    client, _ = listener.accept()
                except socket.timeout:
                    continue
                threading.Thread(target=self.handle, args=(client,), daemon=True).start()


def _agent_endpoint(agent: dict[str, object]) -> tuple[int, str]:
    return 0, str(agent["address"])


def _client(agent: dict[str, object], timeout: float = 20) -> Client:
    client = Client(str(agent["host"]), int(agent["port"]), timeout=timeout)
    client.address = str(agent["address"])
    client.outer = (2, client.address, client.outer[2], client.outer[3])
    return client


def _exchange(agent: dict[str, object], target: dict[str, object], content: str,
              payload: dict[str, object]) -> tuple[dict[str, object], dict[str, object]]:
    client = _client(agent)
    try:
        meta, body = client.exchange(_agent_endpoint(target), content,
                                     json.dumps(payload).encode("utf-8"))
        client.finish()
    finally:
        client.close()
    return meta, json.loads(body)


def _inspect(agent: dict[str, object]) -> dict[str, object]:
    meta, body = _exchange(agent, agent, INSPECT, {})
    if meta["content"] != SNAPSHOT:
        raise AssertionError(f"INSPECT returned {meta['content']}")
    return body


def _wait_failure(agent: dict[str, object], timeout: float) -> dict[str, object]:
    deadline = time.monotonic() + timeout
    samples: list[dict[str, object]] = []
    while time.monotonic() < deadline:
        snapshot = _inspect(agent)
        samples.append(snapshot["transport"])
        if snapshot["transport"]["failures"]["count"] == 1:
            snapshot["r9_wait_samples"] = samples
            return snapshot
        time.sleep(0.1)
    raise TimeoutError(f"ingress did not retain one transport failure: {samples[-3:]}")


def run(config_path: Path, output: Path) -> int:
    config = json.loads(config_path.read_text(encoding="utf-8"))
    ingress, target = config["ingress"], config["target"]
    report: dict[str, object] = {
        "schema": 1, "source": config.get("source"), "binaries": config.get("binaries"),
        "ingress": ingress, "target": target, "started_unix_ms": int(time.time() * 1000),
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    client = _client(ingress, timeout=30)
    try:
        first = client.send(_agent_endpoint(target), INSPECT, b"{}")
        second = client.send(_agent_endpoint(target), INSPECT, b"{}")
        first_meta, first_body = client.receive()
        if first_meta["correlation"] != first or first_meta["content"] != SNAPSHOT:
            raise AssertionError("first physical response did not match the first request")
        report["first_response"] = {"meta": first_meta, "nodes": json.loads(first_body)["nodes"]}

        retained = _wait_failure(ingress, 15)
        failures = retained["transport"]["failures"]
        row = failures["failure_ids"][0]
        if row["state"] != "uncertain" or failures["retained_event_bytes"] <= 0:
            raise AssertionError(f"unexpected retained failure: {failures}")
        receiver_before = _inspect(target)
        receipts_before = receiver_before["transport"]["receipts"]
        # The inspection itself is an acknowledged hop record while its snapshot
        # is produced, so the lost receipt must appear in addition to that record.
        if receipts_before["accepted"] < 2 or receipts_before["reserved_bytes"] <= 0:
            raise AssertionError(f"receiver did not pin the lost receipt: {receipts_before}")
        report["before_reconcile"] = {"ingress": retained, "receiver": receiver_before}

        reconcile_meta, reconcile = _exchange(ingress, ingress, RECONCILE,
                                               {"failure_id": row["failure_id"]})
        if reconcile_meta["content"] != RESULT or not reconcile.get("ok") or reconcile.get("state") != "accepted_exact":
            raise AssertionError(f"exact reconciliation failed: {reconcile}")
        report["reconcile"] = reconcile

        second_meta, second_body = client.receive()
        if second_meta["correlation"] != second or second_meta["content"] != SNAPSHOT:
            raise AssertionError("queued physical response did not resume in order")
        report["second_response"] = {"meta": second_meta, "nodes": json.loads(second_body)["nodes"]}
        client.finish()
    finally:
        client.close()

    wave = []
    normal = _client(ingress, timeout=30)
    try:
        pending = [normal.send(_agent_endpoint(target), INSPECT, b"{}") for _ in range(8)]
        for expected in pending:
            meta, body = normal.receive()
            if meta["correlation"] != expected or meta["content"] != SNAPSHOT or json.loads(body)["nodes"] != []:
                raise AssertionError("post-reconcile wave changed order or response semantics")
            wave.append(meta)
        normal.finish()
    finally:
        normal.close()

    final_ingress, final_target = _inspect(ingress), _inspect(target)
    if final_ingress["transport"]["failures"]["count"] != 0:
        raise AssertionError("ingress failure was not retired")
    # Only the observation request itself may be present while its snapshot is made.
    if final_target["transport"]["receipts"]["records"] != 1:
        raise AssertionError(f"receiver receipt was not released: {final_target['transport']['receipts']}")
    report.update({"wave": wave, "final": {"ingress": final_ingress, "target": final_target},
                   "finished_unix_ms": int(time.time() * 1000), "passed": True})
    output.write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(json.dumps({"passed": True, "wave": len(wave), "failure_state": "uncertain",
                      "reconcile": reconcile["state"]}))
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="mode", required=True)
    proxy = sub.add_parser("proxy")
    proxy.add_argument("--listen-host", default="0.0.0.0")
    proxy.add_argument("--listen-port", type=int, required=True)
    proxy.add_argument("--backend-host", default="127.0.0.1")
    proxy.add_argument("--backend-port", type=int, required=True)
    proxy.add_argument("--output", type=Path, required=True)
    execute = sub.add_parser("run")
    execute.add_argument("config", type=Path)
    execute.add_argument("output", type=Path)
    args = parser.parse_args()
    if args.mode == "proxy":
        ReceiptDropProxy(args.listen_host, args.listen_port, args.backend_host,
                         args.backend_port, args.output).serve()
        return 0
    return run(args.config, args.output)


if __name__ == "__main__":
    raise SystemExit(main())
