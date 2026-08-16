# Tools

Programs that drive the layer rather than being part of it. Neither is linked
into the agent.

| Path | What it is |
| --- | --- |
| `drive/` | OUTER. Creates nodes, loads them, runs inferences and prints a verdict. Built on the same core as an agent, which is the point: nothing in the layer distinguishes the thing that asks from the things that answer. |
| `link/` | A relay that carries frames badly on purpose — latency, jitter, width, stalls, and a cut. Used as a library by the tests and as a binary between machines. |

An earlier `controller/` and `scripts/` lived here and are gone. The first was
a client for a participant the protocol no longer has; the second drove the
runtime that the v6 core replaced. Their measurements are still recorded in
[`docs/runtime-evidence.md`](../docs/runtime-evidence.md), which is what those
runs were for.
