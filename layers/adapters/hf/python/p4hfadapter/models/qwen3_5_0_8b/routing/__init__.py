"""Route issued steps and releases through the ordered Qwen process nodes."""

from pathlib import Path
import uuid
from p4hfadapter.models.qwen3_5_0_8b.processes import Peer


class Pipeline:
    def __init__(self, root: Path, plan_path: Path, plan, directory: Path, output: Path, timeout=120):
        if any(node.host != "local" for node in plan.nodes):
            raise ValueError("this standalone launcher executes host=local only; remote plans can be inspected")
        self.peers, self.reports, self.trace = [], [], []
        self.run_id = uuid.uuid4().hex
        try:
            for node in plan.nodes:
                peer = Peer(root, plan_path, node, directory, output, self.run_id, timeout)
                self.peers.append(peer)
                ready, tensor = peer.exchange()
                if ready.get("op") != "ready" or tensor is not None:
                    raise ValueError("invalid worker readiness")
                self.reports.append({**ready["report"], "pid": peer.process.pid})
        except BaseException as error:
            error.partial_nodes = self.reports
            try:
                self.close()
            except Exception as cleanup:
                error.cleanup_error = f"{type(cleanup).__name__}: {cleanup}"
            raise

    def step(self, session_id, issue, position, tokens):
        tensor = tokens
        for peer in self.peers:
            reply, tensor = peer.exchange({"op": "step", "session_id": session_id,
                                           "issue": issue, "position": position}, tensor)
            if (reply.get("op"), reply.get("session_id"), reply.get("issue"), reply.get("position")) != (
                    "result", session_id, issue, position):
                raise ValueError("mismatched stage completion")
            state = reply["state"]
            if state["issue"] != issue + 1 or state["position"] != position + tokens.shape[1]:
                raise ValueError("stage progress disagrees with issued membership")
            self.trace.append({"node_id": peer.node.node_id, "session_id": session_id, **state})
        return tensor

    def release(self, session_id):
        for peer in self.peers:
            reply, tensor = peer.exchange({"op": "release", "session_id": session_id})
            if reply.get("op") != "released" or reply.get("session_id") != session_id or tensor is not None:
                raise ValueError("invalid release confirmation")
            self.trace.append({"node_id": peer.node.node_id, "release": reply["state"]})

    def shutdown(self):
        for peer in self.peers:
            reply, tensor = peer.exchange({"op": "shutdown"})
            if reply.get("op") != "stopped" or tensor is not None:
                raise ValueError("invalid shutdown confirmation")
            if peer.process.wait(timeout=5) != 0:
                raise RuntimeError("worker failed during shutdown")

    def close(self):
        errors = []
        for peer in reversed(self.peers):
            try:
                peer.close()
            except Exception as error:
                errors.append(f"{peer.node.node_id}: {type(error).__name__}: {error}")
        if errors:
            raise RuntimeError("; ".join(errors))
