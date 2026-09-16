# Tools

> Document status (2026-09-06): **Component guide**. This is an API and structure guide for this path. The legacy service path and the current event path are distinguished by their actual callers.
> Current goals, status and ordering follow the [execution roadmap](../docs/distributed-batching-roadmap.md); document authority and reading paths follow the [document map](../docs/document-map.md).

Programs that drive the layer rather than being part of it. Neither is linked
into the agent.

| Path | What it is |
| --- | --- |
| `drive/` | OUTER. Creates nodes, loads them, runs inferences and prints a verdict. Built on the same core as an agent, which is the point: nothing in the layer distinguishes the thing that asks from the things that answer. |
| `link/` | A relay that carries frames badly on purpose — latency, jitter, width, stalls, and a cut. Used as a library by the tests and as a binary between machines. |
| [model-loading/](model-loading/README.md) | P4 OUTER model-loading module: inventory, typed planner, CLI, independent reference, tests and local evidence. |

Build on remote hosts. The development desktop build path is disabled by default after
repeated hard power losses during consecutive cold Rust builds. A separately authorized
emergency run must set `P4_ALLOW_LOCAL_BUILD=1` and use
`tools/scripts/invoke-local-resource-policy.ps1 Build -- cargo ...`; the wrapper then caps
Cargo, CMake and Rust tests at the smaller of eight jobs or 25% of logical CPUs. This is a
conservative concurrency ceiling, not a wall-power measurement. Run unavoidable local
inference through `invoke-local-resource-policy.ps1 Inference -- COMMAND`; it fails unless
the designated RTX 3090 is present and exposes only that GPU through
`CUDA_VISIBLE_DEVICES`.

`model-loading/cli/collect-inventory.ts` sends an ordinary agent INSPECT to every
configured address and atomically writes per-machine timestamped JSON plus
`latest.json`. A failed required machine remains in the fleet failure list and
makes the command fail after the partial evidence has been saved. The placement
policy consumes these records; it does not maintain a handwritten GPU table.

An earlier `controller/` and `scripts/` lived here and are gone. The first was
a client for a participant the protocol no longer has; the second drove the
runtime that the v6 core replaced. Their measurements are still recorded in
[`docs/runtime-evidence.md`](../docs/runtime-evidence.md), which is what those
runs were for.
