# llama.cpp, where it is llama.cpp's alone

현재 native/llama/backend의 책임과 잦은 upstream 변경을 흡수할 강제 경계는
[계층 격리 계약](../../../docs/layer-isolation-contract.md)을 따른다.

> 문서 지위 (2026-09-06): **구성요소 안내**. 해당 경로의 API·구조 안내다. 과거 service 경로와 현재 event 경로는 실제 호출자로 구분한다.
> 현재 목표·상태·순서는 [실행 로드맵](../../../docs/distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](../../../docs/document-map.md)를 따른다.

| Path | What it is |
| --- | --- |
| `served/` | `Distribution::Internal`. Crate `p4-llamacpp-served`. Stock `llama-server` over its HTTP surface — one process holding the whole model, one entry point, so a chain over it is one link. Starts and stops that process when the plan carries a `start`. |
| `staged/` | `Distribution::Staged`. Splits the model across machines with the llama.cpp adapter owning layer placement, mutable memory residency, graph cuts, and physical UBATCH scheduling. P4 only delivers independent submissions. The private llama.cpp dependency is isolated in an ordered patch series and a verified patched worktree. See [`docs/llamacpp-stage-memory.md`](../../../docs/llamacpp-stage-memory.md). |
| `upstream/` | llama.cpp's own repository, cloned by `npm run p4:upstream` and ignored by ours. Used only by `staged/`. |

They are shapes rather than backends, which is why they share a folder. A
sibling of `mock/` called `pipeline` read like a third backend; it was the same
llama.cpp arranged differently, and the clone it patches belongs to llama.cpp
rather than to one arrangement of it.

## vLLM and SGLang are registered here, and should not be

`served/` carries three names because three backends answer a similar wire, and
that was taken as one coupling worth writing once. It was a mistake. Adapters
diverge as their backends move; sharing one implementation across them means
fixing one and breaking another.

The divergence did not wait. Two branches already exist — vLLM alone checks the
model name, and `launch` refuses vLLM and SGLang outright, so **starting and
killing the backend, which the operating guidelines make mandatory, works for
one tenant of three.** The other two get an error where the capability should
be. Apparent duplication would have been cheaper than that.

Each backend gets its own adapter. llama.cpp leaves first, into `adapter/`
beside `origin/`; vLLM and SGLang separate at that moment rather than being left
behind as a two-tenant crate.

None of this is visible above the boundary. The agent speaks P4 over a socket
and nothing else — no HTTP, and no service API of any kind. What an adapter does
privately with its backend is that adapter's business.

## The cost of each

`served/` pays nothing when upstream moves. It speaks two endpoints —
`/v1/models` and `/v1/chat/completions` — which vLLM and SGLang serve as well;
a newer llama.cpp is a new binary and nothing else. Tests enforce that: no
build script, no `-sys` or bindgen dependency, no `llama.h`, no `ggml`, exactly
two dependencies, exactly those two paths. The property is the reason the
adapter is worth having in this form, so it is checked rather than intended.

`staged/` pays a rebase for every upstream commit it follows. `compat/` holds
the series for each, four revisions deep already. That is the price of owning
the boundary between two halves of a model, and it is only worth paying where
the alternative — one machine holding the whole thing — does not fit.

## Which to reach for

`served/` unless the model does not fit on one machine. A chain over `served/`
is one link; a chain over `staged/` is as many links as there are pieces.
