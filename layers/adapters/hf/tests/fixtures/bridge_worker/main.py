"""Effect-counting pipe fixture. No torch/model; failures are selected before LOAD."""
import json
import os
import struct
import sys
import time
import subprocess


def read():
    header = sys.stdin.buffer.read(16)
    if len(header) != 16:
        raise EOFError()
    data = sys.stdin.buffer.read(struct.unpack("!Q", header[8:])[0])
    n, = struct.unpack("!I", data[:4])
    return json.loads(data[4:4+n]), data[4+n:]


def write(meta, body=b""):
    header = json.dumps(meta, separators=(",", ":")).encode()
    data = struct.pack("!I", len(header)) + header + body
    sys.stdout.buffer.write(b"P4HF\x01\0\0\0" + struct.pack("!Q", len(data)) + data)
    sys.stdout.buffer.flush()


init, _ = read()
mode = init["config"].get("mode", "normal")
if mode == "ready_mismatch":
    init["identity"] = {}
if mode == "stderr":
    sys.stderr.buffer.write(b"x" * 200000)
    sys.stderr.buffer.flush()
report={"pid":os.getpid()}
if mode=="tree":
    child=subprocess.Popen([sys.executable,"-c","import time; time.sleep(60)"],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,creationflags=0x08000000 if os.name=="nt" else 0)
    report["child_pid"]=child.pid
write({"op":"ready", "protocol":2,"identity":init["identity"],"bundle_sha256":init["bundle_sha256"],"report":report})
active = set()
calls = 0
while True:
    meta, body = read()
    job = meta["job"]
    if mode == "hang":
        time.sleep(60)
    if mode == "death":
        sys.exit(7)
    if mode == "partial":
        sys.stdout.buffer.write(b"P4HF\x01\0\0\0" + struct.pack("!Q", 500) + b"abc")
        sys.stdout.buffer.flush()
        sys.exit(8)
    if mode in ("magic", "version", "reserved", "length"):
        header = bytearray(b"P4HF\x01\0\0\0" + struct.pack("!Q", 100))
        header[{"magic":0,"version":4,"reserved":5,"length":8}[mode]] = 9
        sys.stdout.buffer.write(header)
        sys.stdout.buffer.flush()
        sys.exit(9)
    if mode == "identity":
        job["issue"] += 1
    kind = job["kind"]
    if kind == "step":
        active.add(job["request"])
        calls += 1
    elif kind in ("release", "cancel"):
        active.remove(job["request"])
    elif kind in ("epoch", "unload") and active:
        write({"job":job,"ok":False,"disposition":"rejected","error":"live state"})
        continue
    report = {"calls":calls,"active":len(active)}
    if mode == "lifecycle_frame" and kind == "unload":
        report["padding"] = "x" * 700
    write({"job":job,"ok":True,"active":len(active),"report":report},
          body if kind == "step" else b"")
    if kind == "unload":
        if mode == "unload_exit_error":
            sys.exit(17)
        break
