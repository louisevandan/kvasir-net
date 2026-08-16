# Running it

## An agent per machine

```bash
p4-agent 0.0.0.0:19311 192.168.0.6
```

The second argument is what this agent calls itself. Peers put it in an
envelope, so it must be the address they can reach rather than the interface it
bound. Omitted, it uses the bound address — fine on one machine, wrong across a
fleet.

On start it prints what it can serve:

```
P4_AGENT_READY address=tcp://192.168.0.6:19311 adapters=[mock, mock-instant]
```

A node named against a backend not in that list is refused, because a placement
mistake is the caller's to fix and a silent fallback would hide it.

## Driving a fleet

```bash
p4-drive LISTEN CHAIN REQUESTS TOKENS [ADAPTER]

p4-drive 0.0.0.0:19310 192.168.0.6:19311,192.168.0.26:19311 1000 64 mock
```

`CHAIN` is the stage order. The driver creates a node per stage, loads each,
runs the requests, and prints a verdict:

```
P4_DRIVE_RESULT requests=1000 tokens_each=64
  completed=1000 failed=0 unanswered=0 routes=1006
  tokens=63000 elapsed_ms=1211 frames_per_second=52846
  [pass] every request answered
  [pass] no request failed
  [pass] every stream in order
  [pass] one terminal per route
```

Throughput is reported but is not one of the claims. This layer does not own
throughput, and a figure from a simulated backend would say nothing about a
real one.

`P4_DRIVE_CEILING` sets the concurrency each load declares; it defaults to 32.

## Watching an agent

```bash
P4_AGENT_STATS=1 p4-agent 0.0.0.0:19311
```

```
P4_AGENT_DEPTH control=0 prefill=812 decode=44 response=0 nodes=196
P4_AGENT_TRAFFIC forwarded=38409 consumed=3 to_nodes=38403 unrouted=0
P4_AGENT_NODE node=tail-0 received=12801 queued=12801 claimed=12800 hops=745 …
```

Read the first two numbers together. A deep node queue beside shallow lanes
puts a slowdown below the adapter; deep lanes beside an idle node put it here.
The per-node counts exist because a frame that goes missing leaves no trace in
a depth reading — depth only shows what is still waiting.

## Which backends a build carries

Registered in [`entrypoints/agent/src/adapters`](../entrypoints/agent/src/adapters).
`mock` reproduces the measured shape — cost by chain position, prefill dearer
than a lap — so a fleet can be loaded anywhere. `mock-instant` answers with no
delay, for proving routing and ordering at rates a timed backend would hide.

A node's name tells a staged backend which position it plays: `stage-N` is an
intermediate stage, `tail-N` is the end of a chain, and any other name is a
backend that spreads the model itself.
