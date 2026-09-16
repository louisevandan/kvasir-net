# llama.cpp, where it is llama.cpp's alone

The current native/llama/backend responsibilities, and the enforced boundary that absorbs frequent upstream changes,
follow the [layer isolation contract](../../../docs/layer-isolation-contract.md).

> Document status (2026-09-06): **Component guide**. This is an API and structure guide for this path. The legacy service path and the current event path are distinguished by their actual callers.
> Current goals, status and ordering follow the [execution roadmap](../../../docs/distributed-batching-roadmap.md); document authority and reading paths follow the [document map](../../../docs/document-map.md).

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
