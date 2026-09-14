"""One P4-owned stage. No sockets, peer processes or model scheduling here."""
import hashlib
import json
import os
from pathlib import Path
import platform
import sys

from p4hfadapter.integration.packet import pack, unpack
from p4hfadapter.transport.framing.limits import FrameLimits
from p4hfadapter.transport.framing.receiving import FrameReceiver
from p4hfadapter.transport.framing.sending import FrameSender


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def serve(root, build_label):
    sender = FrameSender(sys.stdout.buffer, FrameLimits(32*1024*1024))
    receiver = FrameReceiver(sys.stdin.buffer, FrameLimits(32*1024*1024))
    init, body = unpack(receiver.receive())
    if body or init.get("op") != "initialize" or init.get("protocol") != 2:
        raise ValueError("incompatible initialization")
    actual_bundle = hashlib.sha256((root / "bundle.json").read_bytes()).hexdigest()
    if actual_bundle != init["bundle_sha256"]:
        raise ValueError("bundle changed before worker initialization")
    config, identity = init["config"], init["identity"]
    if hashlib.sha256(canonical(config)).hexdigest() != identity["config_sha256"]:
        raise ValueError("configuration identity mismatch")
    from p4hfadapter.models.qwen3_5_0_8b.configuration import parse_plan
    from p4hfadapter.models.qwen3_5_0_8b.evidence import verify_checkpoint
    from p4hfadapter.models.qwen3_5_0_8b.loading import load_stage
    from p4hfadapter.models.qwen3_5_0_8b.epoch import EpochSessions
    from p4hfadapter.models.qwen3_5_0_8b.cache_export import export
    from safetensors.torch import load, save
    plan = parse_plan(config["plan"])
    directory = Path(config["model_dir"])
    artifact = verify_checkpoint(root, directory)
    if identity["model_revision"] != artifact["revision"] or identity["model_sha256"] != artifact["files"]["model.safetensors-00001-of-00001.safetensors"]["sha256"]:
        raise ValueError("model identity mismatch")
    if identity["tokenizer_sha256"] != artifact["files"]["tokenizer.json"]["sha256"]:
        raise ValueError("tokenizer identity mismatch")
    if identity["plan_sha256"] != hashlib.sha256(canonical(config["plan"])).hexdigest() or identity["dtype"] != plan.dtype:
        raise ValueError("plan/dtype identity mismatch")
    if identity["boundary"] != "qwen3.5-text-safetensors-v2" or identity["operations"] != ["step", "release", "cancel", "epoch", "cache", "unload"]:
        raise ValueError("unsupported boundary/operation identity")
    node = plan.nodes[identity["index"]]
    if node.node_id != config["node_id"]:
        raise ValueError("stage identity mismatch")
    sender = FrameSender(sys.stdout.buffer, FrameLimits(init["frame_bytes"]))
    receiver = FrameReceiver(sys.stdin.buffer, FrameLimits(init["frame_bytes"]))
    stage, report = load_stage(directory, node, plan.dtype)
    epochs = EpochSessions(stage, plan)
    report.update(pid=os.getpid(), host=platform.node(), python=sys.executable,
                  build_label=build_label)
    sender.send(pack({"op":"ready", "protocol":2, "identity":identity, "bundle_sha256":actual_bundle, "report":report}))
    while (packet := receiver.receive()) is not None:
        meta, body = unpack(packet)
        job = meta["job"]
        uncertain = False
        try:
            if set(job) != {"generation", "epoch", "serial", "kind", "request", "issue", "position"} or job["generation"] != identity["generation"]:
                raise ValueError("invalid job identity")
            kind = job["kind"]
            if kind == "epoch":
                if body:
                    raise ValueError("epoch must not have tensor body")
                epochs.advance(job["epoch"])
                output, state = b"", {"epoch":epochs.epoch, "active_sessions":0}
            else:
                epochs.validate_epoch(job["epoch"])
                sessions = epochs.sessions
                if kind == "step":
                    tensors = load(body)
                    if set(tensors) != {"tensor"}:
                        raise ValueError("invalid Qwen boundary members")
                    sessions.validate_step(job["request"], job["issue"], job["position"], tensors["tensor"])
                    # Validation has no cache effects; failures during execution require abort.
                    uncertain = True
                    tensor, state = sessions.step(job["request"], job["issue"], job["position"], tensors["tensor"])
                    output = save({"tensor":tensor})
                elif kind in ("release", "cancel"):
                    if body:
                        raise ValueError("release/cancel has a tensor body")
                    current = sessions.active.get(job["request"])
                    if current is None or (current.issue, current.position) != (job["issue"], job["position"]):
                        raise ValueError("release/cancel cutoff mismatch")
                    uncertain = True
                    state = sessions.release(job["request"])
                    state["cancelled"] = kind == "cancel"
                    output = b""
                elif kind == "cache":
                    current = sessions.active.get(job["request"])
                    if body or current is None or (current.issue, current.position) != (job["issue"], job["position"]):
                        raise ValueError("cache snapshot identity mismatch")
                    output, state = export(current.cache, node.start), {"layers":[node.start,node.end]}
                elif kind == "unload":
                    if body or sessions.active:
                        raise ValueError("unload with active state or tensor body")
                    sender.send(pack({"job":job,"ok":True,"active":0,"report":{"active_sessions":0}}))
                    return
                else:
                    raise ValueError("unsupported operation")
            sender.send(pack({"job":job,"ok":True,"report":state}, output))
        except Exception as error:
            sender.send(pack({"job":job,"ok":False,"disposition":"uncertain" if uncertain else "rejected",
                              "error":f"{type(error).__name__}: {error}"}))
            if uncertain:
                # Keep the pipe alive for an explicit supervisor abort, but never run more model work.
                return
