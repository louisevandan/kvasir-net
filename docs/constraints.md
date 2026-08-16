# Invariants

What breaks if each goes. Most of these are here because they broke once.

| Invariant | What went wrong without it |
| --- | --- |
| A socket reader only enqueues | Handlers ran inside the read loop, putting every handler's duration there. |
| A worker never waits | Waiting in a worker turns a bounded queue back into an unbounded one. |
| A node's queue holds the long work | Otherwise GPU time appears as agent queue depth and nothing can be attributed. |
| A node runs one hop at a time | It starts the next only on seeing the previous end; a timer instead would overlap them. |
| One route, one worker | Frames of a route on different workers race, and a caller sees P4 reordering its stream. |
| Lane preference is bounded | Strict priority is a veto: a lap never dispatched is a request that never finishes. |
| A node's select is not biased | Preferring completions starved arrivals entirely; they sat in a channel, in no queue, invisible. |
| Nothing on the send path drops | A reply has no reply address, so a refused reply cannot be answered — it just vanishes. |
| The declared ceiling is a ceiling | Concurrency above it is not safe on the current native runtime. |
| Windows are composed next to the node | A gate further from the work reported a limit its arrivals disagreed with. |
| Connection, in-flight and depth are sized apart | One constant did all three; narrowing the release width narrowed the others silently. |
| Load reports per stage | One total hid a non-final stage reserving the whole model. |
| Only a chain's end produces a token | A middle stage counting makes an n-stage chain emit n tokens per lap. |
| The requested token count bounds the ring | A backend that never reports a stop otherwise laps forever, across every node at once. |
| Expired work is answered, not dropped | A caller waiting for a terminal that never comes is what a leaked route looks like. |
| A peer connection is checked before use | A dead socket accepts a write into its buffer, so the first frame after a restart is lost. |
| An advertised address is one a peer can reach | Two agents both calling themselves `127.0.0.1` finish the prefill and stop after one token: the lap resolves to whichever machine holds the frame. |
| An advertised hint is parsed, not pasted | Appending the bound port to a hint that carried one produced `HOST:52001:52001`, which parses, resolves to nothing, and reports itself ready. |

## Boundaries

`layers/protocol` knows no backend and no message vocabulary — an envelope and
a frame. `layers/adapter` depends on nothing at all, including the protocol: an
adapter reaching for a P4 type is reaching past its own contract.
`layers/agent` routes opaque bodies and reads them only through the `Payload`
seam a deployment supplies.

Hidden state never crosses the adapter interface. That transfer stays inside a
backend under either distribution, which is why a chain over a self-contained
backend is one link rather than a special case.

## What this layer does not have

No TLS, authentication or authorisation. A self-describing address is trusted
because the network is. Use inside a trusted network only; anything public must
authenticate before it becomes a frame.

No durable state. An agent that restarts has no nodes, and OUTER holds the
record that says what it should have.
