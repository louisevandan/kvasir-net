#!/usr/bin/env python3
"""Autonomous expert-worker loop (M3 of docs/design/moe-expert-sharding.md).

Drives a node's full market participation without an operator:

    volunteer -> download the assigned expert shard -> serve it ->
    heartbeat coverage (and restart the worker if it dies)

  expert-worker-agent.py \
    --hub https://gate.kvasir-ai.net --model Qwen3.5-122B-A10B-Q4_K_M.gguf \
    --worker-bin ~/linkcpp/build-.../linkcpp-expert-worker \
    --n-embd 3072 --n-layer 48 --n-expert 256 \
    [--max-experts 128] [--port 52800] [--slice-dir ~/models/shards] \
    [--relay wss://gate.kvasir-ai.net/api/expert-relay --session S] \
    [--worker-id ID] [--cpu] [--once]

--hub may be the gateway (which passes the participation API through to the
LAN-only hub) or a directly reachable hub. Auth comes from --token or the
LINKCPP_NODE_TOKEN env (sent as both bearer and service-token headers; the
hub decides what it honors). With --relay/--session the agent also keeps a
dial-out bridge up via expert-relay-dial.py (same directory).

Stdlib only. If no segment is assigned (coverage at target) the agent keeps
serving what it already has, or polls until the market needs it.
"""

import argparse
import json
import os
import signal
import subprocess
import sys
import time
import urllib.request


def log(msg):
    print("[expert-agent] " + msg, file=sys.stderr, flush=True)


class Api:
    def __init__(self, base, token):
        self.base = base.rstrip("/")
        self.token = token

    def _req(self, method, path, body=None):
        # a real UA: CDN bot rules block the default Python-urllib agent
        headers = {"content-type": "application/json",
                   "user-agent": "kvasir-expert-agent/1.0"}
        if self.token:
            headers["x-linkcpp-service-token"] = self.token
            headers["authorization"] = "Bearer " + self.token
        return urllib.request.Request(self.base + path,
                                      data=json.dumps(body).encode() if body is not None else None,
                                      headers=headers, method=method)

    def call(self, method, path, body=None, timeout=30):
        with urllib.request.urlopen(self._req(method, path, body), timeout=timeout) as r:
            return json.load(r)

    def download(self, path, dst, timeout=600):
        with urllib.request.urlopen(self._req("GET", path), timeout=timeout) as r, open(dst, "wb") as f:
            while True:
                chunk = r.read(1 << 20)
                if not chunk:
                    break
                f.write(chunk)
        return os.path.getsize(dst)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--hub", required=True, help="gateway or hub base URL (http/https)")
    ap.add_argument("--model", required=True)
    ap.add_argument("--worker-bin", required=True)
    ap.add_argument("--n-embd", type=int, required=True)
    ap.add_argument("--n-layer", type=int, required=True)
    ap.add_argument("--n-expert", type=int, required=True)
    ap.add_argument("--max-experts", type=int, default=128)
    ap.add_argument("--port", type=int, default=52800)
    ap.add_argument("--slice-dir", default=os.path.expanduser("~/models/shards"))
    ap.add_argument("--worker-id", default="agent-" + str(os.getpid()))
    ap.add_argument("--token", default=os.environ.get("LINKCPP_NODE_TOKEN", ""))
    ap.add_argument("--relay", default="", help="wss://.../api/expert-relay to dial out through")
    ap.add_argument("--session", default="", help="relay session name (with --relay)")
    ap.add_argument("--cpu", action="store_true")
    ap.add_argument("--heartbeat-sec", type=float, default=60.0)
    ap.add_argument("--poll-sec", type=float, default=120.0, help="re-volunteer poll when unassigned")
    ap.add_argument("--once", action="store_true", help="stop after the worker exits once")
    args = ap.parse_args()

    api = Api(args.hub, args.token)
    os.makedirs(args.slice_dir, exist_ok=True)
    here = os.path.dirname(os.path.abspath(__file__))

    assignment = None      # {"layer": L, "experts": [a, b]}
    worker = None
    bridge = None

    def stop(*_):
        for p in (worker, bridge):
            if p and p.poll() is None:
                p.terminate()
        sys.exit(0)
    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)

    while True:
        # 1) ask the market what it needs most (keep serving if nothing new)
        if assignment is None:
            try:
                a = api.call("POST", "/api/expert-volunteer",
                             {"model": args.model, "max_experts": args.max_experts})
            except Exception as exc:
                log("volunteer failed: %r — retrying in %.0fs" % (exc, args.poll_sec))
                time.sleep(args.poll_sec)
                continue
            if not a.get("assigned"):
                log("coverage at target — polling again in %.0fs" % args.poll_sec)
                time.sleep(args.poll_sec)
                continue
            assignment = {"layer": int(a["layer"]), "experts": [int(x) for x in a["experts"]]}
            log("assigned layer %d experts [%d,%d) (scarcity %s)"
                % (assignment["layer"], assignment["experts"][0], assignment["experts"][1],
                   a.get("scarcity")))

        layer, (e0, e1) = assignment["layer"], assignment["experts"]
        slice_path = os.path.join(args.slice_dir,
                                  "%s.L%d_e%03d-%03d.gguf" % (os.path.basename(args.model), layer, e0, e1))

        # 2) fetch the shard once (cached across restarts)
        if not os.path.exists(slice_path):
            path = ("/api/proxy/models/%s/expert-shard?layers=%d:%d&experts=%d:%d"
                    % (args.model, layer, layer + 1, e0, e1))
            log("downloading shard -> " + slice_path)
            try:
                n = api.download(path, slice_path)
                log("shard ready (%d bytes)" % n)
            except Exception as exc:
                log("shard download failed: %r — retrying in %.0fs" % (exc, args.poll_sec))
                try:
                    os.remove(slice_path)
                except OSError:
                    pass
                time.sleep(args.poll_sec)
                continue

        # 3) serve it
        cmd = [args.worker_bin, "--model", slice_path, "--serve", str(args.port),
               "--layer", str(layer), "--n-embd", str(args.n_embd)]
        if args.cpu:
            cmd.append("--cpu")
        log("starting worker: " + " ".join(cmd))
        worker = subprocess.Popen(cmd)
        if args.relay and args.session:
            bridge = subprocess.Popen(
                [sys.executable, os.path.join(here, "expert-relay-dial.py"),
                 "--mode", "worker", "--hub", args.relay, "--session", args.session,
                 "--local", "127.0.0.1:%d" % args.port, "--token", args.token])

        # 4) heartbeat while the worker lives
        while worker.poll() is None:
            try:
                api.call("POST", "/api/expert-coverage", {
                    "worker_id": args.worker_id, "model": args.model,
                    "n_layer": args.n_layer, "n_expert": args.n_expert,
                    "segments": [[layer, e0, e1]],
                    "url": ("relay:" + args.session) if args.session else ("tcp:%d" % args.port),
                }, timeout=15)
            except Exception as exc:
                log("heartbeat failed: %r" % (exc,))
            time.sleep(args.heartbeat_sec)

        log("worker exited (rc=%s)" % worker.returncode)
        if bridge and bridge.poll() is None:
            bridge.terminate()
        if args.once:
            return
        time.sleep(5)      # crash loop guard; keep the same assignment and restart


if __name__ == "__main__":
    main()
