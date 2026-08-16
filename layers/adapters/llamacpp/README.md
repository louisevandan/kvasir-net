# llama.cpp, where it is llama.cpp's alone

| Path | What it is |
| --- | --- |
| `staged/` | `Distribution::Staged`. Splits the model across machines with P4 owning the boundary between layer ranges, which needs llama.cpp internals it does not expose — so an ordered patch series, and the script that materialises a verified patched worktree. |
| `upstream/` | llama.cpp's own repository, cloned by `npm run p4:upstream` and ignored by ours. Used only by `staged/`. |

They are shapes rather than backends, which is why they share a folder. A
sibling of `mock/` called `pipeline` read like a third backend; it was the same
llama.cpp arranged differently, and the clone it patches belongs to llama.cpp
rather than to one arrangement of it.

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
