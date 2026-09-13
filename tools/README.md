# Tools

> 문서 지위 (2026-09-06): **구성요소 안내**. 해당 경로의 API·구조 안내다. 과거 service 경로와 현재 event 경로는 실제 호출자로 구분한다.
> 현재 목표·상태·순서는 [실행 로드맵](../docs/distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](../docs/document-map.md)를 따른다.

Programs that drive the layer rather than being part of it. Neither is linked
into the agent.

| Path | What it is |
| --- | --- |
| `drive/` | OUTER. Creates nodes, loads them, runs inferences and prints a verdict. Built on the same core as an agent, which is the point: nothing in the layer distinguishes the thing that asks from the things that answer. |
| `link/` | A relay that carries frames badly on purpose — latency, jitter, width, stalls, and a cut. Used as a library by the tests and as a binary between machines. |
| `cluster-inference/` | OUTER TypeScript policy. Persists agent hardware discovery and chooses memory tiers and contiguous model-stage cuts from measured memory plans and service profiles. |

An earlier `controller/` and `scripts/` lived here and are gone. The first was
a client for a participant the protocol no longer has; the second drove the
runtime that the v6 core replaced. Their measurements are still recorded in
[`docs/runtime-evidence.md`](../docs/runtime-evidence.md), which is what those
runs were for.
