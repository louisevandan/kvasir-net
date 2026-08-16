# The wire

P4B1 v6. Two things travel: an envelope every hop reads, and a body only its
destination does.

That split is the design. A relay forwards by copying bytes it never decoded,
so forwarding costs the same whatever the message is and a new kind of message
cannot make a relay heavier.

## Frame

```
 0      4   5      8         12        16
 +------+---+------+---------+---------+----------------+---------------+
 | P4B1 | 6 | zero | env_len | body_len|   envelope     |     body      |
 +------+---+------+---------+---------+----------------+---------------+
```

Sixteen-byte header, little-endian lengths. A reader takes the header, learns
the total length, and knows it has a whole frame without decoding any of it.

Envelope at most 256 KiB, body at most 1 MiB. A version that is not 6 is
refused rather than read — a stale binary on a benchmark host must fail loudly
instead of misreading a v6 payload.

## Envelope

| Field | Meaning |
| --- | --- |
| `target` | `tcp://host:port`. Where this goes. |
| `recipient` | `Agent` or `Node(id)`. Who consumes it once the address matched. |
| `lane` | `Control`, `Prefill`, `Decode`, `Response`. Which queue it waits in. |
| `route` | Transport correlation for one exchange. |
| `deadline_unix_ms` | Absolute; zero disables. Checked at hop boundaries. |
| `reply_to` | Where an answer goes. Absent when nobody is listening. |
| `chain` | The ordered nodes an inference travels. Absent on control messages. |

**The address is the identity.** There is no agent id to resolve, which is what
lets a relay decide without consulting anything — the decision that has to stay
stateless for relaying to be free.

**The lane is carried, not derived.** A receiver places a frame without opening
it, which is what lets a socket reader do nothing but enqueue.

## Chain

A chain is an ordered list of links and a position. Each link names one
executable target completely:

```
(address, node, binding, generation)
```

All four are needed. Without the generation a request can reach a node that has
since been rebound and execute against the wrong materialisation.

The **whole** chain travels, not the remainder. That costs frame size and buys
two things: a node can say where in the order it sat, and a failed request can
be retried without reconstructing what it was meant to visit.

A chain of one is valid and is how a backend that spreads a model internally
participates — vLLM and SGLang expose one entry point, so a chain over one is
one link.

Advancing stops at the end rather than wrapping. A decode lap restarts
explicitly, so finishing a pass and continuing generation stay distinguishable
events.

## Two decisions

Every frame that reaches an agent is decided twice, and never more:

```
is the target mine?
  no  → forward it whole; a peer and OUTER are the same case
  yes → agent or node?
          agent → node create and delete, inference intake, hardware
          node  → model load and unload, prefill and decode
```

The second decision has exactly two answers because an agent owns exactly one
kind of internal entity. A third would mean something acquired state it should
not have.

## Bodies

Opaque here. What a body means belongs to whoever sends and receives it; the
standard vocabulary is in [`layers/service`](../layers/service), and the core
reads a body only through the `Payload` seam a deployment supplies.
