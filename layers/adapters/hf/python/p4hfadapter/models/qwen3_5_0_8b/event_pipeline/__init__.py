"""Model controller transport: each step crosses P4 brokers, never worker pipes."""

import hashlib
import json
from pathlib import Path

from p4hfadapter.integration.lifecycle import (
    ADAPTER_KIND,
    NODE_LIFECYCLE_RESULT,
    NODE_LOAD,
    NODE_UNLOAD,
    decode_result,
    encode_request,
)
from p4hfadapter.integration.packet import pack, unpack


COMMAND = "application/vnd.p4.hf.command-v2"
RESULT = "application/vnd.p4.hf.result-v2"
CAPACITIES = {
    "queue_capacity": 1,
    "completion_capacity": 1,
    "retained_capacity": 2,
    "retained_bytes": 128 * 1024 * 1024,
}


def canonical(value):
    return json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode()


class Pipeline:
    def __init__(
        self,
        client,
        plan,
        model_dir,
        python,
        bundle,
        nodes,
        generation=1,
        deployments=None,
    ):
        self.client, self.nodes, self.generation = client, nodes, generation
        self.epoch, self.serial = 1, 0
        self.reports, self.trace = [], []
        self.ready, self.owned = [], []
        self.last = {}
        self.bundle = str(Path(bundle).resolve())
        artifact = json.loads(
            (
                Path(bundle).parent
                / "manifests/qwen3_5_0_8b/artifact/identity.json"
            ).read_text(encoding="utf-8")
        )
        if len(nodes) != len(plan["nodes"]) or (
            deployments is not None and len(deployments) != len(nodes)
        ):
            raise ValueError("deployment/topology length mismatch")
        deployments = deployments or [{} for _ in nodes]
        if any(
            set(deployment) - {"python", "bundle", "model_dir"}
            for deployment in deployments
        ):
            raise ValueError("unsupported deployment field")

        bundle_sha256 = hashlib.sha256(Path(bundle).read_bytes()).hexdigest()
        launches = []
        for index, _node in enumerate(nodes):
            deployment = deployments[index]
            config = {
                "plan": plan,
                "model_dir": deployment.get("model_dir", str(Path(model_dir).resolve())),
                "node_id": plan["nodes"][index]["node_id"],
            }
            identity = {
                "generation": generation,
                "index": index,
                "nodes": nodes,
                "config_sha256": hashlib.sha256(canonical(config)).hexdigest(),
                "model_revision": artifact["revision"],
                "model_sha256": artifact["files"][
                    "model.safetensors-00001-of-00001.safetensors"
                ]["sha256"],
                "tokenizer_sha256": artifact["files"]["tokenizer.json"]["sha256"],
                "plan_sha256": hashlib.sha256(canonical(plan)).hexdigest(),
                "dtype": plan["dtype"],
                "boundary": "qwen3.5-text-safetensors-v2",
                "operations": [
                    "step",
                    "release",
                    "cancel",
                    "epoch",
                    "cache",
                    "unload",
                ],
            }
            launches.append(
                {
                    "python": deployment.get("python", str(python)),
                    "bundle": deployment.get("bundle", self.bundle),
                    "bundle_sha256": bundle_sha256,
                    "config": config,
                    "identity": identity,
                    "frame_bytes": 32 * 1024 * 1024,
                    "scratch_bytes": 128 * 1024 * 1024,
                    "stderr_bytes": 65536,
                    "timeout_ms": 120000,
                }
            )

        try:
            for index, launch in enumerate(launches):
                self.owned.append(index)
                lifecycle, reply, _ = self.lifecycle(
                    index,
                    "load",
                    {
                        "op": "load",
                        "generation": generation,
                        "launch": launch,
                        "nodes": nodes,
                        "index": index,
                    },
                )
                if lifecycle["resource_state"] == "absent":
                    self.owned.remove(index)
                if (
                    lifecycle["status"] != "succeeded"
                    or lifecycle["resource_state"] != "present"
                ):
                    raise RuntimeError(
                        f"P4 LOAD failed: lifecycle={lifecycle} adapter={reply}"
                    )
                if reply.get("ok") is not True or "ready" not in reply:
                    raise ValueError("HF LOAD terminal omitted readiness")
                self.ready.append(index)
                self.reports.append(reply["ready"]["report"])
        except Exception as first_error:
            try:
                self.close()
            except Exception as cleanup_error:
                raise RuntimeError(
                    f"{first_error}; lifecycle rollback failed: {cleanup_error}"
                ) from first_error
            raise

    def target(self, index):
        node = self.nodes[index]
        return (1, node["agent"], node["node"], node["generation"])

    def agent(self, index):
        return (0, self.nodes[index]["agent"])

    def lifecycle(self, index, operation, meta, body=b""):
        node = self.nodes[index]
        content = NODE_LOAD if operation == "load" else NODE_UNLOAD
        payload = encode_request(
            operation,
            node,
            COMMAND,
            pack(meta, body),
            CAPACITIES if operation == "load" else None,
        )
        envelope, raw = self.client.exchange(
            self.agent(index), content, payload, ADAPTER_KIND
        )
        if (
            envelope["source"] != self.agent(index)
            or envelope["content"] != NODE_LIFECYCLE_RESULT
        ):
            raise ValueError("node lifecycle response envelope mismatch")
        lifecycle, opaque = decode_result(raw, operation, node, RESULT)
        reply, output = unpack(opaque) if opaque else ({}, b"")
        self.trace.append(
            {
                "input": meta,
                "lifecycle": lifecycle,
                "result": reply,
                "body_bytes": len(output),
                "source": envelope["source"],
            }
        )
        return lifecycle, reply, output

    def command(self, index, meta, body=b"", require=True):
        kind = meta.get("job", {}).get("kind")
        if meta.get("op") == "load" or kind in ("unload", "abort"):
            raise ValueError("LOAD, UNLOAD and abort require the Agent lifecycle path")
        envelope, raw = self.client.exchange(
            self.target(index), COMMAND, pack(meta, body), ADAPTER_KIND
        )
        reply, output = unpack(raw)
        if require and reply.get("ok") is not True:
            raise RuntimeError(f"HF command failed: {reply}")
        if reply.get("ok") and "job" in meta:
            expected = self.target(index if kind == "cache" else len(self.nodes) - 1)
            if envelope["source"] != expected or reply.get("job") != meta["job"]:
                raise ValueError("tail/identity approval mismatch")
        self.trace.append(
            {
                "input": meta,
                "result": reply,
                "body_bytes": len(output),
                "source": envelope["source"],
            }
        )
        return reply, output

    def job(self, kind, request="", issue=0, position=0, epoch=None):
        return {
            "generation": self.generation,
            "epoch": self.epoch if epoch is None else epoch,
            "serial": self.serial + 1,
            "kind": kind,
            "request": request,
            "issue": issue,
            "position": position,
        }

    def step(self, request, issue, position, tokens):
        from safetensors.torch import load, save

        job = self.job("step", request, issue, position)
        reply, body = self.command(
            0, {"job": job, "receipts": []}, save({"tensor": tokens})
        )
        if len(reply["receipts"]) != len(self.nodes):
            raise ValueError("incomplete stage settlement")
        for receipt in reply["receipts"]:
            if (
                receipt["report"]["issue"] != issue + 1
                or receipt["report"]["position"] != position + tokens.shape[1]
            ):
                raise ValueError("stage progress mismatch")
        self.serial += 1
        self.last[request] = (issue + 1, position + tokens.shape[1])
        return load(body)["tensor"]

    def release(self, request, cancel=False):
        issue, position = self.last[request]
        job = self.job("cancel" if cancel else "release", request, issue, position)
        reply, body = self.command(0, {"job": job, "receipts": []})
        if body or len(reply["receipts"]) != len(self.nodes):
            raise ValueError("incomplete release chain")
        self.serial += 1
        del self.last[request]

    def advance(self):
        if self.last:
            raise ValueError("controller epoch before all stage releases")
        self.command(
            0, {"job": self.job("epoch", epoch=self.epoch + 1), "receipts": []}
        )
        self.epoch += 1
        self.serial += 1

    def cancel(self, request):
        self.release(request, cancel=True)

    def cache(self, index, request):
        issue, position = self.last[request]
        _, body = self.command(
            index,
            {"job": self.job("cache", request, issue, position), "receipts": []},
        )
        return body

    def shutdown(self):
        errors = []
        for index in list(self.ready):
            try:
                lifecycle, reply, body = self.lifecycle(
                    index,
                    "unload",
                    {"job": self.job("unload"), "receipts": []},
                )
                if body or reply.get("ok") is not True:
                    raise ValueError("HF UNLOAD terminal is incomplete")
                if lifecycle["resource_state"] == "absent":
                    self.ready.remove(index)
                    self.owned.remove(index)
                if (
                    lifecycle["status"] != "succeeded"
                    or lifecycle["resource_state"] != "absent"
                ):
                    raise RuntimeError(f"P4 UNLOAD failed: {lifecycle}")
            except Exception as error:
                errors.append(str(error))
        if errors:
            raise RuntimeError("; ".join(errors))

    def close(self):
        errors = []
        for index in reversed(list(self.owned)):
            try:
                abort = {
                    "job": {
                        "generation": self.generation,
                        "epoch": 0,
                        "serial": 0,
                        "kind": "abort",
                        "request": "",
                        "issue": 0,
                        "position": 0,
                    },
                    "receipts": [],
                }
                lifecycle, _, body = self.lifecycle(index, "unload", abort)
                if body:
                    raise ValueError("HF abort terminal carried a body")
                if lifecycle["resource_state"] == "absent":
                    self.owned.remove(index)
                    if index in self.ready:
                        self.ready.remove(index)
                else:
                    raise RuntimeError(f"P4 abort did not prove absence: {lifecycle}")
            except Exception as error:
                errors.append(str(error))
        if errors:
            raise RuntimeError("; ".join(errors))
