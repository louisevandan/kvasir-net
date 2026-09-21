#!/usr/bin/env python3
"""Serve expert slices of a GGUF to the bridge, and nothing else.

The model is a 122 GB file on the machine that runs the p4 agents. The bridge is
on another host with 96 GB of disk, so it cannot hold the model and cannot cut
shards out of it. This sits beside the file and hands out the bytes a remote
worker needs; the bridge relays them and never parses what it carries.

## Why the request is semantic, not a byte range

The obvious design is "give me offset X, length N", and it is the wrong one. An
endpoint that reads any offset of any configured file is a file-disclosure
oracle the moment its token leaks or its host is reachable from somewhere it
should not be. So a caller asks for `layer=7&expert_begin=0&expert_end=32` and
this decides what that means. A request that names a layer with no experts, or
an expert past the end, is refused rather than clamped: clamping would return
the wrong weights and look like success.

## Why it parses the GGUF itself

The byte map could have been passed in from the bridge, which already has one
(gguf-topology.mjs writes it into the catalog). That would make this a dumb
reader and avoid a second parser — but it would also mean this process trusts a
map it cannot check against the file it is reading, so a stale catalog would
serve silently wrong weights. Reading the header here costs a few seconds at
startup and makes the file its own authority.

The duplicate parser is a real cost. `--dump-map` exists so the two can be
compared: it prints the same shape gguf-topology.mjs does, and the two must
agree on every offset for the same file.

## Response framing

    [4 bytes big-endian: manifest length][manifest JSON][gate][up][down]

The manifest says which tensors follow, in which order, their ggml type, dims
and per-expert size, so a worker can rebuild the tensors without having been
told anything out of band. Concatenated rather than multipart because the
consumer is a program, the parts are megabytes, and a length-prefixed header is
one obvious thing to get right.

Usage:
    shard-server.py --model <id>=<path.gguf> [--model ...]
                    [--host 127.0.0.1] [--port 42300]
                    [--max-experts 64]
    shard-server.py --model <id>=<path.gguf> --dump-map

The token is read from P4_SHARD_TOKEN. Without it the server refuses to start:
an open one would hand the model out to anyone who can reach the port.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import struct
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse, parse_qs

# ggml type -> (name, elements per block, bytes per block). Not defaulted: an
# unknown type is a quantisation this was never checked against, and guessing a
# size produces offsets that look plausible and address the wrong weights.
GGML_TYPES = {
    0: ("F32", 1, 4), 1: ("F16", 1, 2),
    2: ("Q4_0", 32, 18), 3: ("Q4_1", 32, 20),
    6: ("Q5_0", 32, 22), 7: ("Q5_1", 32, 24),
    8: ("Q8_0", 32, 34), 9: ("Q8_1", 32, 40),
    10: ("Q2_K", 256, 84), 11: ("Q3_K", 256, 110), 12: ("Q4_K", 256, 144),
    13: ("Q5_K", 256, 176), 14: ("Q6_K", 256, 210), 15: ("Q8_K", 256, 292),
    16: ("IQ2_XXS", 256, 66), 17: ("IQ2_XS", 256, 74), 18: ("IQ3_XXS", 256, 98),
    19: ("IQ1_S", 256, 50), 20: ("IQ4_NL", 32, 18), 21: ("IQ3_S", 256, 110),
    22: ("IQ2_S", 256, 82), 23: ("IQ4_XS", 256, 136),
    24: ("I8", 1, 1), 25: ("I16", 1, 2), 26: ("I32", 1, 4), 27: ("I64", 1, 8),
    28: ("F64", 1, 8), 29: ("IQ1_M", 256, 56), 30: ("BF16", 1, 2),
}

EXPERT_TENSORS = ("ffn_gate_exps", "ffn_up_exps", "ffn_down_exps")

# GGUF pads every tensor to this, and so does the shard. The earlier raw
# framing did not: its three tensors started at offsets congruent to 14 mod
# 32, because a JSON manifest of whatever length sat in front of them. numpy
# does not care; a ggml consumer reading with aligned loads does.
GGUF_ALIGNMENT = 32


class Reader:
    """A cursor over the header bytes, so the parse reads as the format does."""

    def __init__(self, fh):
        self.fh = fh

    def take(self, n: int) -> bytes:
        b = self.fh.read(n)
        if len(b) != n:
            raise ValueError("header ended mid-value")
        return b

    def u32(self): return struct.unpack("<I", self.take(4))[0]
    def i32(self): return struct.unpack("<i", self.take(4))[0]
    def u64(self): return struct.unpack("<Q", self.take(8))[0]
    def i64(self): return struct.unpack("<q", self.take(8))[0]
    def string(self): return self.take(self.u64()).decode("utf-8", "replace")

    def value(self, t: int):
        if t == 0: return self.take(1)[0]
        if t == 1: return struct.unpack("<b", self.take(1))[0]
        if t == 2: return struct.unpack("<H", self.take(2))[0]
        if t == 3: return struct.unpack("<h", self.take(2))[0]
        if t == 4: return self.u32()
        if t == 5: return self.i32()
        if t == 6: return struct.unpack("<f", self.take(4))[0]
        if t == 7: return self.take(1)[0] != 0
        if t == 8: return self.string()
        if t == 9:
            elem = self.u32()
            n = self.u64()
            return [self.value(elem) for _ in range(n)]
        if t == 10: return self.u64()
        if t == 11: return self.i64()
        if t == 12: return struct.unpack("<d", self.take(8))[0]
        raise ValueError(f"unknown metadata value type {t}")


def tensor_bytes(name: str, dims: list[int], type_id: int) -> int:
    known = GGML_TYPES.get(type_id)
    if not known:
        raise ValueError(f"{name}: unsupported ggml type {type_id}")
    _, block_elems, block_bytes = known
    elems = 1
    for d in dims:
        elems *= d
    if elems % block_elems != 0:
        raise ValueError(f"{name}: {elems} elements is not a whole number of blocks")
    return elems // block_elems * block_bytes


class Model:
    """One GGUF, reduced to the expert slices that can be served from it."""

    def __init__(self, model_id: str, path: str):
        self.id = model_id
        self.path = path
        self.lock = threading.Lock()
        with open(path, "rb") as fh:
            r = Reader(fh)
            if r.take(4) != b"GGUF":
                raise ValueError(f"{path}: not a GGUF file")
            r.u32()                       # version
            tensor_count = r.u64()
            kv_count = r.u64()
            meta = {}
            for _ in range(kv_count):
                key = r.string()
                meta[key] = r.value(r.u32())
            tensors = []
            for _ in range(tensor_count):
                name = r.string()
                dims = [r.u64() for _ in range(r.u32())]
                type_id = r.u32()
                offset = r.u64()
                tensors.append((name, dims, type_id, offset))
            alignment = meta.get("general.alignment", 32)
            header_end = fh.tell()
        data_start = -(-header_end // alignment) * alignment
        self.data_start = data_start

        arch = meta.get("general.architecture")
        self.arch = arch
        self.n_expert = meta.get(f"{arch}.expert_count")
        if not self.n_expert:
            raise ValueError(f"{path}: {arch} declares no expert_count")
        self.n_embd = meta.get(f"{arch}.embedding_length")
        self.n_layer = meta.get(f"{arch}.block_count")
        # The expert FFN width, which is not the dense one: this model's dense
        # blocks are 11264 wide and its experts 1280.
        self.n_ff_expert = meta.get(f"{arch}.expert_feed_forward_length")

        self.slice: dict[int, dict] = {}
        for name, dims, type_id, offset in tensors:
            if not name.startswith("blk."):
                continue
            parts = name.split(".")
            if len(parts) != 4 or parts[3] != "weight" or parts[2] not in EXPERT_TENSORS:
                continue
            layer = int(parts[1])
            if dims[-1] != self.n_expert:
                raise ValueError(f"{name}: last dim {dims[-1]} is not expert_count {self.n_expert}")
            total = tensor_bytes(name, dims, type_id)
            if total % self.n_expert:
                # An expert's slice would end inside a quantisation block, and a
                # cut there hands the worker half a block it cannot decode.
                raise ValueError(f"{name}: {total} bytes does not divide into {self.n_expert} experts")
            self.slice.setdefault(layer, {})[parts[2]] = {
                "offset": data_start + offset,
                "per_expert": total // self.n_expert,
                "type": type_id,
                "type_name": GGML_TYPES[type_id][0],
                "dims": dims,
            }

        # A layer missing any of the three cannot be computed by a worker, so it
        # is not servable and is not advertised.
        self.expert_layers = sorted(
            l for l, t in self.slice.items() if all(w in t for w in EXPERT_TENSORS))
        if not self.expert_layers:
            raise ValueError(f"{path}: no complete routed-expert layers")
        first = self.slice[self.expert_layers[0]]
        self.bytes_per_expert = sum(first[w]["per_expert"] for w in EXPERT_TENSORS)
        self.digest = self._digest(header_end, data_start)

    def _digest(self, header_end: int, data_start: int) -> str:
        """A cheap identity for the checkpoint these weights come from.

        Not a hash of the file. Reading 122 GB to answer one shard request is
        not a trade anyone would take, and doing it once at startup would still
        delay serving by minutes for a number that only has to distinguish one
        checkpoint from another.

        So: the size, the whole header — every tensor's name, shape, type and
        offset — and two megabytes sampled from the head and tail of the tensor
        data. The header alone would not do it, because a requantisation of the
        same model keeps every shape and can keep every offset; the sampled data
        is what separates those. It is a identity check, not an integrity check,
        and it is named `digest` rather than `sha256` so nobody reads it as one.
        """
        h = hashlib.sha256()
        size = os.path.getsize(self.path)
        h.update(str(size).encode())
        with open(self.path, "rb") as fh:
            h.update(fh.read(header_end))
            fh.seek(data_start)
            h.update(fh.read(1 << 20))
            fh.seek(max(data_start, size - (1 << 20)))
            h.update(fh.read(1 << 20))
        return h.hexdigest()

    def plan(self, layer: int, begin: int, end: int) -> list[dict]:
        """The byte ranges that make up experts [begin, end) of `layer`."""
        if layer not in self.expert_layers:
            raise KeyError(f"layer {layer} holds no routed experts")
        if not (0 <= begin < end <= self.n_expert):
            raise KeyError(f"experts [{begin}, {end}) fall outside [0, {self.n_expert})")
        out = []
        for which in EXPERT_TENSORS:
            t = self.slice[layer][which]
            out.append({
                "tensor": which,
                "start": t["offset"] + begin * t["per_expert"],
                "bytes": (end - begin) * t["per_expert"],
                "per_expert": t["per_expert"],
                "type": t["type"],
                "type_name": t["type_name"],
                "dims": t["dims"],
            })
        return out

    def dump_map(self) -> dict:
        return {
            "id": self.id,
            "n_embd": self.n_embd,
            "n_layer": self.n_layer,
            "n_expert": self.n_expert,
            "expert_layers": self.expert_layers,
            "bytes_per_expert": self.bytes_per_expert,
            "expert_slice": {str(k): v for k, v in sorted(self.slice.items())},
        }



# ---- GGUF writing ----------------------------------------------------------
# The inverse of the parse above. A shard goes out as a GGUF because the thing
# that will compute with it is a ggml program, and a GGUF is what ggml opens —
# `gguf_init_from_file` instead of a bespoke header, an offset table and three
# assumptions about axis order. It also carries its own description, which the
# raw framing did not: three defects Astra found reading the earlier format are
# structural here rather than documented.

GGUF_U32, GGUF_U64, GGUF_STR = 4, 10, 8


def _kv(key: str, vtype: int, value) -> bytes:
    out = _gstr(key) + struct.pack("<I", vtype)
    if vtype == GGUF_STR:
        return out + _gstr(value)
    if vtype == GGUF_U32:
        return out + struct.pack("<I", value)
    if vtype == GGUF_U64:
        return out + struct.pack("<Q", value)
    raise ValueError(f"no writer for metadata type {vtype}")


def _gstr(text: str) -> bytes:
    raw = text.encode("utf-8")
    return struct.pack("<Q", len(raw)) + raw


def build_shard_gguf(model: "Model", layer: int, begin: int, end: int) -> tuple[bytes, list[dict]]:
    """The header for a shard, and the byte ranges whose contents follow it.

    Returns the complete GGUF up to the start of tensor data, so the caller can
    write it and then stream the ranges straight off the model file.
    """
    plan = model.plan(layer, begin, end)
    n_local = end - begin

    meta = [
        ("general.architecture", GGUF_STR, model.arch),
        ("general.alignment", GGUF_U32, GGUF_ALIGNMENT),
        # What this is a shard OF. Without it a shard is three anonymous slabs:
        # a reader cannot tell experts [0,32) of layer 7 from [64,96) of layer 9,
        # and the local->global mapping a router needs is not recoverable.
        ("kvasir.expert_shard.model", GGUF_STR, model.id),
        ("kvasir.expert_shard.layer", GGUF_U32, layer),
        ("kvasir.expert_shard.expert_begin", GGUF_U32, begin),
        ("kvasir.expert_shard.expert_end", GGUF_U32, end),
        # The model's own totals, so a shard can be placed in the whole without
        # fetching anything else.
        ("kvasir.expert_shard.n_expert_total", GGUF_U32, model.n_expert),
        # Also under the architecture-prefixed names below, which is where GGUF
        # convention puts them — but finding those means reading
        # general.architecture first and building the key from it. A shard
        # consumer is not a model loader and should not have to know the
        # architecture to learn the two numbers its graph needs, so they are
        # repeated here where they can be looked up directly. The Windows
        # worker was deriving n_embd from a tensor's first dimension for want
        # of this.
        ("kvasir.expert_shard.n_embd", GGUF_U32, model.n_embd),
        ("kvasir.expert_shard.n_ff", GGUF_U32, model.n_ff_expert),
        # Which checkpoint these weights came from. Name and shape are not
        # identity: a requantisation of the same model has both and different
        # numbers, and mixing two of those silently produces garbage nobody can
        # trace. See Model.digest for exactly what this covers.
        ("kvasir.expert_shard.source_digest", GGUF_STR, model.digest),
        ("step35.embedding_length" if model.arch == "step35"
         else f"{model.arch}.embedding_length", GGUF_U32, model.n_embd),
        (f"{model.arch}.block_count", GGUF_U32, model.n_layer),
        (f"{model.arch}.expert_count", GGUF_U32, model.n_expert),
    ]

    header = b"GGUF" + struct.pack("<IQQ", 3, len(plan), len(meta))
    for key, vtype, value in meta:
        header += _kv(key, vtype, value)

    # Tensor infos. The stored expert dimension is n_local, NOT the model's 288:
    # writing the source count would mean the file describes 288 experts while
    # holding a handful, and a reader that believes the header walks off the end
    # of the data. The global range lives in the metadata above instead.
    offset = 0
    infos = b""
    for part in plan:
        dims = list(part["dims"][:-1]) + [n_local]
        infos += _gstr(f"blk.{layer}.{part['tensor']}.weight")
        infos += struct.pack("<I", len(dims))
        for d in dims:
            infos += struct.pack("<Q", d)
        infos += struct.pack("<II", part["type"], 0)[:4]          # type
        infos += struct.pack("<Q", offset)                        # offset in data
        part["data_offset"] = offset
        offset += part["bytes"]
        offset = -(-offset // GGUF_ALIGNMENT) * GGUF_ALIGNMENT    # pad to alignment

    header += infos
    pad = (-len(header)) % GGUF_ALIGNMENT
    header += b"\x00" * pad
    return header, plan


class Handler(BaseHTTPRequestHandler):
    server_version = "kvasir-shard/1"
    models: dict[str, Model] = {}
    token: str = ""
    max_experts: int = 64

    def log_message(self, fmt, *args):     # one line per request, to stderr
        sys.stderr.write("%s - %s\n" % (self.address_string(), fmt % args))

    def _json(self, status: int, body: dict):
        raw = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def _authed(self) -> bool:
        seen = self.headers.get("x-kvasir-service-token", "")
        # Constant-time: the token is the only thing between the public side of
        # the relay and this file.
        import hmac
        return bool(seen) and hmac.compare_digest(seen, self.token)

    def do_GET(self):
        url = urlparse(self.path)
        if url.path == "/health":
            return self._json(200, {"ok": True, "models": sorted(self.models)})
        if not self._authed():
            return self._json(401, {"error": "service token required"})
        if url.path == "/topology":
            q = parse_qs(url.query)
            model = self.models.get((q.get("model") or [""])[0])
            if not model:
                return self._json(404, {"error": "unknown model"})
            return self._json(200, model.dump_map())
        if url.path != "/shard":
            return self._json(404, {"error": f"no such endpoint {url.path}"})

        q = parse_qs(url.query)
        def num(key):
            try:
                return int((q.get(key) or [""])[0])
            except ValueError:
                raise KeyError(f"{key} must be an integer")
        try:
            model = self.models.get((q.get("model") or [""])[0])
            if not model:
                return self._json(404, {"error": "unknown model"})
            layer, begin, end = num("layer"), num("expert_begin"), num("expert_end")
            if end - begin > self.max_experts:
                return self._json(413, {
                    "error": f"at most {self.max_experts} experts per request",
                })
            plan = model.plan(layer, begin, end)
        except KeyError as e:
            return self._json(400, {"error": str(e)})

        try:
            header, plan = build_shard_gguf(model, layer, begin, end)
        except KeyError as e:
            return self._json(400, {"error": str(e)})
        # Each tensor is padded up to the alignment the header declares, so the
        # offsets in it are the offsets a reader will compute.
        body = 0
        for p in plan:
            body = p["data_offset"] + p["bytes"]
            body = -(-body // GGUF_ALIGNMENT) * GGUF_ALIGNMENT

        self.send_response(200)
        self.send_header("content-type", "application/octet-stream")
        self.send_header("content-length", str(len(header) + body))
        self.send_header("x-kvasir-shard-digest", model.digest)
        self.end_headers()
        self.wfile.write(header)

        # One descriptor per request: pread would let threads share one, but a
        # separate open is simpler to reason about and the cost is a syscall.
        written = 0
        with open(model.path, "rb") as fh:
            for p in plan:
                if p["data_offset"] > written:
                    self.wfile.write(b"\x00" * (p["data_offset"] - written))
                    written = p["data_offset"]
                fh.seek(p["start"])
                left = p["bytes"]
                while left:
                    chunk = fh.read(min(1 << 20, left))
                    if not chunk:
                        # The file shrank or was replaced under us. The response
                        # is already committed, so the only honest thing left is
                        # to cut the connection rather than pad with zeros.
                        raise IOError(f"{model.path}: short read serving {p['tensor']}")
                    self.wfile.write(chunk)
                    left -= len(chunk)
                    written += len(chunk)
        if body > written:
            self.wfile.write(b"\x00" * (body - written))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", action="append", default=[], metavar="ID=PATH")
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=42300)
    ap.add_argument("--max-experts", type=int, default=64)
    ap.add_argument("--dump-map", action="store_true")
    args = ap.parse_args()

    models = {}
    for spec in args.model:
        if "=" not in spec:
            sys.exit(f"--model wants ID=PATH, got {spec}")
        model_id, path = spec.split("=", 1)
        models[model_id] = Model(model_id, path)
        m = models[model_id]
        print(f"{model_id}: {len(m.expert_layers)} expert layers "
              f"({m.expert_layers[0]}..{m.expert_layers[-1]}), "
              f"{m.n_expert} experts, {m.bytes_per_expert} bytes each",
              file=sys.stderr)
    if not models:
        sys.exit("nothing to serve: pass --model ID=PATH")

    if args.dump_map:
        json.dump({k: v.dump_map() for k, v in models.items()}, sys.stdout, indent=2)
        sys.stdout.write("\n")
        return

    token = os.environ.get("P4_SHARD_TOKEN", "").strip()
    if not token:
        sys.exit("P4_SHARD_TOKEN is not set; refusing to serve the model unauthenticated")

    Handler.models = models
    Handler.token = token
    Handler.max_experts = args.max_experts
    httpd = ThreadingHTTPServer((args.host, args.port), Handler)
    print(f"serving shards on {args.host}:{args.port}", file=sys.stderr)
    httpd.serve_forever()


if __name__ == "__main__":
    main()
