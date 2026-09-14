"""Own and supervise local Qwen stage processes through bounded IPC."""

import os
import queue
import subprocess
import sys
import threading

from p4hfadapter.models.qwen3_5_0_8b.boundary import pack, unpack
from p4hfadapter.transport.framing.limits import FrameLimits
from p4hfadapter.transport.framing.receiving import FrameReceiver
from p4hfadapter.transport.framing.sending import FrameSender


class Peer:
    def __init__(self, root, plan_path, node, directory, output, run_id, timeout):
        self.node, self.run_id, self.timeout = node, run_id, timeout
        self.log = (output / f"{node.node_id}.stderr.log").open("wb")
        self.thread = None
        command = [sys.executable, "-B", str(root / "scripts/models/qwen3_5_0_8b/cli/run.py"), "worker",
                   "--plan", str(plan_path), "--model-dir", str(directory), "--node", node.node_id, "--run-id", run_id]
        try:
            self.process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.log,
                                            cwd=root, env=dict(os.environ, PYTHONPATH=str(root / "python"),
                                                              PYTHONUTF8="1", PYTHONDONTWRITEBYTECODE="1"))
        except BaseException:
            self.log.close()
            raise
        self.sender = FrameSender(self.process.stdin, FrameLimits(32 * 1024 * 1024))
        self.receiver = FrameReceiver(self.process.stdout, FrameLimits(32 * 1024 * 1024))

    def exchange(self, meta=None, tensor=None):
        result = queue.Queue(maxsize=1)
        def io():
            try:
                if meta is not None:
                    self.sender.send(pack({**meta, "run_id": self.run_id}, tensor))
                payload = self.receiver.receive()
                if payload is None:
                    raise RuntimeError(f"{self.node.node_id}: worker EOF")
                result.put((True, unpack(payload)))
            except BaseException as error:
                result.put((False, error))
        self.thread = threading.Thread(target=io, daemon=True)
        self.thread.start()
        try:
            ok, value = result.get(timeout=self.timeout)
        except queue.Empty:
            raise TimeoutError(f"{self.node.node_id}: IPC timeout; execution is uncertain") from None
        self.thread.join()
        if not ok:
            raise value
        reply, output = value
        if reply.get("run_id") != self.run_id or reply.get("op") == "error":
            raise RuntimeError(f"{self.node.node_id}: {reply}")
        return reply, output

    def close(self):
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)
        if self.thread is not None:
            self.thread.join(timeout=5)
        self.process.stdin.close()
        self.process.stdout.close()
        self.log.close()
