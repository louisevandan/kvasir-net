"""A single owned local process for one Qwen stage; errors fence the worker."""

from pathlib import Path
import sys

from p4hfadapter.models.qwen3_5_0_8b.boundary import pack, unpack
from p4hfadapter.models.qwen3_5_0_8b.loading import load_stage
from p4hfadapter.models.qwen3_5_0_8b.state import StageSessions
from p4hfadapter.transport.framing.limits import FrameLimits
from p4hfadapter.transport.framing.receiving import FrameReceiver
from p4hfadapter.transport.framing.sending import FrameSender


def serve(plan, node_id, directory: Path, run_id: str):
    receiver = FrameReceiver(sys.stdin.buffer, FrameLimits(32 * 1024 * 1024))
    sender = FrameSender(sys.stdout.buffer, FrameLimits(32 * 1024 * 1024))
    try:
        node = next(n for n in plan.nodes if n.node_id == node_id)
        stage, report = load_stage(directory, node, plan.dtype)
        sessions = StageSessions(stage, plan)
        sender.send(pack({"op": "ready", "run_id": run_id, "report": report}))
        while (payload := receiver.receive()) is not None:
            meta, tensor = unpack(payload)
            if meta.get("run_id") != run_id:
                raise ValueError("stale worker run identity")
            op = meta.get("op")
            if op == "step":
                if set(meta) != {"op", "run_id", "session_id", "issue", "position"} or tensor is None:
                    raise ValueError("invalid step message")
                output, state = sessions.step(meta["session_id"], meta["issue"], meta["position"], tensor)
                sender.send(pack({**meta, "op": "result", "state": state}, output))
            elif op == "release":
                if set(meta) != {"op", "run_id", "session_id"} or tensor is not None:
                    raise ValueError("invalid release message")
                sender.send(pack({**meta, "op": "released", "state": sessions.release(meta["session_id"])}))
            elif op == "shutdown" and set(meta) == {"op", "run_id"} and tensor is None:
                if sessions.active:
                    raise ValueError("shutdown with live sessions")
                sender.send(pack({"op": "stopped", "run_id": run_id}))
                return 0
            else:
                raise ValueError("unsupported worker operation")
        if sessions.active:
            raise RuntimeError("controller EOF with live sessions")
        return 0
    except Exception as error:
        print(f"QWEN_WORKER_FAILED {type(error).__name__}: {error}", file=sys.stderr, flush=True)
        try:
            sender.send(pack({"op": "error", "run_id": run_id, "error": f"{type(error).__name__}: {error}"}))
        except Exception:
            pass
        return 2
