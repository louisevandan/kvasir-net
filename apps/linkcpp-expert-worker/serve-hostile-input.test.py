"""Send the malformed requests that used to kill the worker, and one good one.

Usage:
    KVASIR_EXPERT_WORKER=<built worker> KVASIR_TEST_SHARD=<2-expert shard.gguf> \
        python3 serve-hostile-input.test.py


Each case is a thing a local user on a shared machine could send to a worker
bound to 127.0.0.1. The worker should close the connection and stay alive; the
last case proves it is still answering afterwards, because a fix that wedges
the accept loop is not a fix.
"""
import os, socket, struct, subprocess, sys, time
import numpy as np

WORKER = os.environ.get("KVASIR_EXPERT_WORKER") or sys.exit("set KVASIR_EXPERT_WORKER to the built worker")
SHARD = os.environ.get("KVASIR_TEST_SHARD") or sys.exit("set KVASIR_TEST_SHARD to a 2-expert shard GGUF")
PORT, N_EMBD = 53911, 4096

proc = subprocess.Popen([WORKER, "--model", SHARD, "--layer", "4", "--n-embd", str(N_EMBD),
                         "--n-ff", "1280", "--serve", str(PORT)],
                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
for _ in range(80):
    try:
        socket.create_connection(("127.0.0.1", PORT), timeout=0.5).close(); break
    except OSError: time.sleep(0.25)
else:
    proc.kill(); raise SystemExit("worker never came up")

def send(name, payload, expect_bytes=0):
    s = socket.create_connection(("127.0.0.1", PORT), timeout=20)
    s.sendall(payload)
    try:
        got = s.recv(expect_bytes or 1)
    except (ConnectionResetError, socket.timeout):
        got = b""
    s.close()
    alive = proc.poll() is None
    ok = (len(got) > 0) if expect_bytes else (len(got) == 0)
    print(f"  {name:38s} replied={len(got):>7d}B  worker_alive={alive}  {'ok' if ok and alive else 'FAIL'}")
    return alive

hidden = (np.random.default_rng(0).standard_normal((4, N_EMBD)) * 0.1).astype(np.float32)

print("the worker holds 2 experts; each of these used to crash it or corrupt its heap")
send("v1 expert id past the shard (id=99)",
     struct.pack("<ii", 1, 4) + hidden.tobytes() + np.full((1, 4), 99, np.int32).tobytes())
send("v1 same expert twice in one token",
     struct.pack("<ii", 2, 4) + hidden.tobytes() + np.zeros((2, 4), np.int32).tobytes())
send("v1 negative n_tokens", struct.pack("<ii", 1, -1))
send("v1 huge n_used", struct.pack("<ii", 1 << 28, 4))
send("v2 ridx writing past out (ridx=1e6)",
     struct.pack("<iii", -2, 2, 1) + np.zeros(N_EMBD * 2, np.float32).tobytes()
     + np.array([1_000_000], np.int32).tobytes() + np.array([0], np.int32).tobytes()
     + np.array([1.0], np.float32).tobytes())
send("v2 negative n_rows", struct.pack("<iii", -2, -5, 1))
send("v2f16 n_pairs enormous", struct.pack("<iii", -3, 2, 1 << 30))

print("and it still serves a legitimate request:")
ok = send("v1 valid, 4 tokens", struct.pack("<ii", 1, 4) + hidden.tobytes()
          + np.zeros((1, 4), np.int32).tobytes(), expect_bytes=N_EMBD * 4 * 4)
proc.terminate()
sys.exit(0 if ok else 1)
