# p4-agent: a direct-mode connection keeps its slot forever after input EOF, even once the peer is gone

**Component:** `entrypoints/agent/src/event_runtime/transport.rs` (p4-kvasir-src)
**Observed:** GB10 #2, agent pid 565476, 2026-09-25 (UTC), on a two-node
Step-3.7-Flash ring driven by p4bridge

**Severity: a leak that accumulates with time, not a one-off incident.**

- Any client that closes without a FINISH frame leaks exactly one slot per
  connection, every time.
- Once 256 have leaked, the agent never accepts a connection again — and it
  reaches that point through ordinary operation. The agent on GB10 #2 had no
  fault of its own. Its last accepted connection came about **41 hours after it
  started** (2026-09-23 04:28Z → 2026-09-24 21:20Z), through ordinary client
  reconnects.
- Existing connections keep working, so nothing looks wrong until something
  needs to reconnect.
- Only a restart recovers it, and a restart kills the agent's loaded stage.

## Summary

In direct (bare-event) mode, an input read error or EOF ends `serve()` with
`P4_EVENT_CONNECTION_STOPPED` (`:1011` → `break`). The writer route is
deliberately kept (`:1076-1078`: *"Input EOF may be a TCP half-close … Keep its
writer route"*). That writer holds the connection's `OwnedSemaphorePermit`
(`writer_with_finish(writer, Arc::clone(&slot), …)`, `:990`) and the socket's
write half.

Nothing reclaims the route after that:

- The peer may send RST or exit. The socket then leaves the kernel TCP table,
  but the fd and the permit stay held.
- No write fails, because nothing is ever written to a route whose channel is
  never used again.
- The route is replaced only if a later connection binds the same channel key.

Clients that use a fresh channel per connection therefore leak one permit per
connection that ends without a FINISH frame. After 256
(`RuntimeLimits.connections`, hardcoded at `mod.rs:48`), `accept()` blocks
forever in `acquire_owned()` (`:902`) *before* `listener.accept()`. New
connections pile up in the kernel accept queue, and the agent logs nothing.

## Why it is hard to see

| Signal | On the wedged agent | Why it misleads |
| --- | --- | --- |
| `ss` CLOSE-WAIT on `:42011` | 4 | leaked sockets that received RST are no longer in the table |
| OPENED lines with no FINISH/STOPPED | 2 | STOPPED is logged, but the slot is **not** released |
| `transport.failures.count` | 8 | only covers path B (preserved failures) |
| `ACCEPT_FAILED`, `panicked`, `TRANSPORT_TASK_FAILED` | 0 | the accept task is alive, just parked on the semaphore |
| **socket fds in `/proc/<pid>/fd`** | **254** (253 unique inodes, 242 in no kernel table) | **the only accurate indicator** |

For comparison: the same agent binary restarted with no clients holds **4**
socket fds (3 unique inodes: 1 listener + 2 unix, plus 1 dup fd), measured
2026-09-25 06:23:49Z. Serving normally with a stage, a bridge and hop peers, it
holds 8 (7 unique, 0 missing from the kernel tables). The healthy head agent on
the other node holds 13 (11 unique, 0 missing).

**Count unique inodes, not fds.** Both agents hold dup fds, so "fds − sockets
visible to `ss`" is non-zero on a healthy agent, and the dup count differs per
machine (2 on one, 1 on the other), so no fixed correction works. The leak is
the set of unique socket inodes held by the agent that appear in no
`/proc/net/{tcp,tcp6,udp,udp6,unix}` table.

### Agent log for the wedged process

Complete from process start; no rotation.

- 339 × `CONNECTION_OPENED` (ids 1–339, contiguous). The last is at
  2026-09-24 21:20Z.
- 1 × `CONNECTION_FINISH`, 336 × `CONNECTION_STOPPED` (308 "unexpected end of
  file", 28 "Connection reset by peer").
- Then nothing. A new connection at 2026-09-25 05:58:09Z reached the kernel
  (LISTEN Recv-Q 1, 87 unread bytes) but was never accepted.

### Slot tally

```
242  hidden (in no kernel table)
  2  ESTABLISHED
  4  CLOSE-WAIT
  1  outbound peer dial  (connect(), :1570, same semaphore, no OPENED line)
───
249  plus ≤ 8 path-B failures  ≈  256
```

## Reproduction

**Not yet run as written.** It is derived from the code path and the production
observation above, because we had no spare agent to test on. The connection
limit is not configurable (`connections: 256` at `mod.rs:48`, no environment
override), so a full reproduction needs 256 iterations. A unit test in
`transport.rs` with a smaller `RuntimeLimits.connections` would be quicker.

1. Start a fresh agent with no clients. Record the baseline
   `B = ls -l /proc/<agent>/fd | grep -c socket:`.
   Expect about 4. A freshly started agent with no clients measured 4 fds:
   3 unique sockets (listener + 2 unix) plus 1 dup.
2. Repeat N times: open TCP, send one direct-mode event that binds a return
   route under a **new** channel name (an INSPECT is enough), read the reply,
   then close the socket *without* sending the 0-length FINISH frame. Killing
   the client process has the same effect and produces an RST.
3. After iteration *k*, expect the socket-fd count to be `B + k` and never to
   fall. After RST, `ss -tan` no longer shows those sockets, so the number of
   unique socket inodes missing from `/proc/net` also grows by *k* (from 0).
4. After `256 − (slots already in use at baseline)` iterations, the next
   connection sits in LISTEN Recv-Q with no `CONNECTION_OPENED`.

**Control:** the same loop, but send FINISH before closing (`:997-1008` detaches
the route, and the ACK is sent at `:480-487`). Expect the socket-fd count to
return to exactly `B` after every iteration, and step 4 never happens.

## Requests

### 1. Reclaim the slot once the peer is gone

A direct-mode route whose socket is fully closed should be reclaimed. Two ways
to detect that:

- The writer task could watch for peer close or error — a zero-byte probe, or
  `writable()` plus `SO_ERROR`.
- Input EOF followed by a read-side error such as *Connection reset* could be
  treated as final rather than as a half-close.

Alternatively, the route could have an idle expiry. Half-close support does not
need to hold the permit after the peer has reset.

### 2. Make the slot count observable, even if Request 1 is fixed

The transport snapshot reports receipts, hop outstanding (whose limit is
`P4_EVENT_HOP_OUTSTANDING`, also 256 by default, which makes it easy to mistake
for the connection limit), transfer counters and failures. **It never says how
many connection permits are held.**

That is why this took a day to find. In the table above, four of the five
signals read "healthy" on a wedged agent. The only accurate one — a socket-fd
count — needs shell access to the agent's host. An operator watching over the
network has nothing to go on.

`Semaphore::available_permits()` is one call. It could be reported in the
INSPECT snapshot as `transport.connections.{limit, available}`.

- A client that keeps a long-lived connection — like our bridge, which
  re-INSPECTs every 15 s over the same socket — could then alert as the count
  falls, without host access.
- It has to be read over an *existing* connection, because a fully exhausted
  agent accepts no new one to INSPECT through. That is one more reason to expose
  it before it reaches zero.

A bounded resource with no reading is why a fixed leak would go unnoticed the
next time it regresses. Please add this even if Request 1 is fixed.

### 3. A question: what is the journal census condition for?

Recovering from this means restarting the agent, which means dealing with its
journal. The agent refuses to start while its journal holds request incarnations
or provisional submissions (`transport.rs:84-95`, test at `tests.rs:920`):
*"recovery census: {requests} request incarnation(s), {submissions} provisional
submission(s)…; request fence and native/KV proof required before restart;
durable intent alone is not a stage ACK"*. **On a ring that has served traffic,
finished requests stay counted, so that condition never clears** (measured
2026-09-23). Taken at face value, a ring that has served even one request can
never restart its agent — which is the only recovery from this defect.

So we moved the journal aside instead, with the owner's explicit approval rather
than a clean census, and said so in our runbook. Afterwards the restarted agent issued its
stage normally and refused nothing — **one data point, not a clearance.**

So: what was the census condition protecting against, and what is the right
check on a ring that has served traffic? A precondition that can never be
satisfied gets quietly dropped, and then whatever it was guarding is unguarded
without anyone deciding to stop guarding it.

## Current workaround (client side)

Every client sends FINISH before closing:

- p4bridge graceful shutdown and connection guard
- `inspect.mjs` and `load.mjs` — `release()` → `client.finish()`

This only covers clients we control, and any abnormal client exit still leaks.
We monitor the agent's socket-fd count and its unique socket inodes missing from
`/proc/net`, not CLOSE-WAIT. We warn at 128 and treat 192 as critical.
