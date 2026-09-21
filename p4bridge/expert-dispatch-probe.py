#!/usr/bin/env python3
"""Stand in for the backbone: dispatch one expert batch to a remote worker.

Everything between a wallet and a GPU on the other side of the world is built —
a device authenticates, is assigned an expert window, downloads exactly those
weights, starts a worker and dials the relay. Then the relay closes with
`1011 upstream unavailable`, because the bridge dials OUT to a coordinator port
and nothing is listening on it. This listens on it.

It is not the backbone. A backbone runs a router, knows which experts a token
selected, and folds what comes back into the next layer. This sends one batch of
hidden states with expert ids of its own choosing and checks the answer against
the same arithmetic done in float64. That is enough to answer the question
nothing else can: does a remote worker, on hardware we do not own, compute the
right thing and get paid for it.

## The direction that surprises people

The worker dials the relay; the bridge accepts that WebSocket and then makes a
TCP connection to `listen_host:listen_port`. So the coordinator is a *server*
here, and the port comes from /api/expert-relay/sessions rather than being
chosen. Connecting to that port instead of listening on it is the mistake this
docstring exists to prevent.

## Wire

    request   int32 n_used, int32 n_tokens
              f32  cur[n_embd * n_tokens]
              i32  sel[n_used * n_tokens]      LOCAL expert ids
    response  f32  out[n_embd * n_used * n_tokens]

`sel` is local to the shard: the worker holds experts [begin, end) of the layer
and indexes them from zero, so a router holding global ids subtracts
`kvasir.expert_shard.expert_begin`.

Usage:
    expert-dispatch-probe.py --bridge http://127.0.0.1:19000 --token-env P4_BRIDGE_TOKEN
                             [--session S] [--tokens 8] [--shard path.gguf]

With --shard it dequantises the same experts and reports the cosine per token;
without it, it reports only that the bytes came back with the right shape.
"""
from __future__ import annotations

import argparse
import json
import os
import socket
import struct
import sys
import urllib.request

import numpy as np


def sessions(bridge: str, token: str) -> list[dict]:
    req = urllib.request.Request(
        f"{bridge.rstrip('/')}/api/expert-relay/sessions",
        headers={"x-kvasir-service-token": token, "user-agent": "kvasir-dispatch-probe/1"},
    )
    with urllib.request.urlopen(req, timeout=15) as reply:
        return json.load(reply)["sessions"]


def dispatch(port: int, host: str, hidden: np.ndarray, sel: np.ndarray, timeout: float) -> np.ndarray:
    """Listen for the bridge, send one batch, read the answer."""
    n_tokens, n_embd = hidden.shape
    n_used = sel.shape[0]

    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind((host, port))
    listener.listen(1)
    listener.settimeout(timeout)
    print(f"  listening on {host}:{port} — waiting for the bridge to dial in", file=sys.stderr)
    try:
        conn, peer = listener.accept()
    except socket.timeout:
        raise SystemExit(
            f"nothing dialled {host}:{port} within {timeout:.0f}s.\n"
            "The worker redials on a backoff, so this usually means its session\n"
            "went stale — check /api/expert-relay/sessions again.")
    print(f"  bridge connected from {peer[0]}:{peer[1]}", file=sys.stderr)
    conn.settimeout(timeout)

    request = (struct.pack("<ii", n_used, n_tokens)
               + hidden.astype(np.float32).tobytes()
               + sel.astype(np.int32).tobytes())
    conn.sendall(request)

    want = n_embd * n_used * n_tokens * 4
    chunks, got = [], 0
    while got < want:
        chunk = conn.recv(min(1 << 20, want - got))
        if not chunk:
            raise SystemExit(f"worker closed after {got} of {want} bytes")
        chunks.append(chunk)
        got += len(chunk)
    conn.close()
    listener.close()
    return np.frombuffer(b"".join(chunks), dtype=np.float32).reshape(n_used, n_tokens, n_embd)


def oracle(shard: str, hidden: np.ndarray, sel: np.ndarray) -> np.ndarray:
    """The same expert FFN in float64, straight from the shard's own weights."""
    from gguf import GGUFReader, quants
    reader = GGUFReader(shard)
    part = {t.name.split(".")[2]: t for t in reader.tensors}
    deq = lambda n: quants.dequantize(part[n].data, part[n].tensor_type).astype(np.float64)
    gate, up, down = deq("ffn_gate_exps"), deq("ffn_up_exps"), deq("ffn_down_exps")
    silu = lambda x: x / (1.0 + np.exp(-x))
    n_used, n_tokens = sel.shape
    out = np.zeros((n_used, n_tokens, hidden.shape[1]))
    for u in range(n_used):
        for t in range(n_tokens):
            e = int(sel[u, t])
            x = hidden[t].astype(np.float64)
            out[u, t] = (silu(x @ gate[e].T) * (x @ up[e].T)) @ down[e].T
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bridge", default="http://127.0.0.1:19000")
    ap.add_argument("--token-env", default="P4_BRIDGE_TOKEN")
    ap.add_argument("--session", default=None, help="default: the only claimed session")
    ap.add_argument("--tokens", type=int, default=8)
    ap.add_argument("--n-used", type=int, default=1)
    ap.add_argument("--shard", default=None, help="the same GGUF, for a float64 check")
    ap.add_argument("--timeout", type=float, default=120.0)
    ap.add_argument("--seed", type=int, default=0)
    args = ap.parse_args()

    token = os.environ.get(args.token_env, "").strip()
    if not token:
        raise SystemExit(f"{args.token_env} is not set")

    claimed = sessions(args.bridge, token)
    if not claimed:
        raise SystemExit("no relay sessions are claimed — nothing has volunteered yet")
    if args.session:
        chosen = next((s for s in claimed if s["session"] == args.session), None)
        if not chosen:
            raise SystemExit(f"no session {args.session}; have {[s['session'] for s in claimed]}")
    elif len(claimed) == 1:
        chosen = claimed[0]
    else:
        raise SystemExit(f"several sessions claimed, pick one: {[s['session'] for s in claimed]}")

    experts = chosen.get("experts") or [0, 1]
    n_local = max(1, experts[1] - experts[0])
    print(f"  session {chosen['session']} · layer {chosen['layer']} · "
          f"experts [{experts[0]},{experts[1]}) · owner {str(chosen['owner'])[:10]}…",
          file=sys.stderr)

    rng = np.random.default_rng(args.seed)
    n_embd = 4096
    hidden = (rng.standard_normal((args.tokens, n_embd)) * 0.1).astype(np.float32)
    sel = rng.integers(0, n_local, size=(args.n_used, args.tokens)).astype(np.int32)

    got = dispatch(chosen["listen_port"], "127.0.0.1", hidden, sel, args.timeout)
    print(f"  received {got.size} floats, shape {got.shape}", file=sys.stderr)

    if not args.shard:
        print("  no --shard given; shape checked, numerics not")
        return
    want = oracle(args.shard, hidden, sel)
    cos = np.array([
        np.dot(want[u, t], got[u, t]) / (np.linalg.norm(want[u, t]) * np.linalg.norm(got[u, t]) + 1e-30)
        for u in range(sel.shape[0]) for t in range(sel.shape[1])
    ])
    print(f"  cosine min {cos.min():.6f} mean {cos.mean():.6f} over {cos.size} outputs")
    print(f"  max relative error {np.max(np.abs(want - got)) / (np.max(np.abs(want)) + 1e-30):.3e}")
    print("  →", "the remote worker computed the right thing"
          if cos.min() > 0.99 else "MISMATCH — do not credit this")


if __name__ == "__main__":
    main()
