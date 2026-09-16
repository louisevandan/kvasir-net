# 2026-09-10 — Defect where plan mode actually allocated recurrent state

Kind: defect cause confirmed, fixed (upstream compat patch), apply/compile verified.
**Follow-up status:** The initial CUDA Release gate passed r96/r256 load, inference and UNLOAD and the rejection of insufficient configurations, and
also checked plan = actual for host/device model/context/compute on each stage of the 2B and 35B models in question.
With the new 0.9.0 Rust binary, r96 passed, but r256 was interrupted by a Windows Update restart, so the final seal is BLOCKED.
The [release gate](2026-09-10-release-gate-v0.9.0.md) owns the latest verdict.
The not-run list below is the record from the time of the original fix; r160 and the other model/backend matrix are for the next version.
Baseline HEAD `ea202000a`. The current work order is owned by the [roadmap](../../../../../../../docs/distributed-batching-roadmap.md).

## Cause — upstream itself handles the two memories differently

The plan path in `stage_memory_plan.cpp` builds the model with `no_alloc = true`. Attention KV behaves as intended,
but recurrent state does not. Placing the upstream sources of the same pin side by side makes this clear.

| | `llama_kv_cache` | `llama_memory_recurrent` (before the fix) |
| --- | --- | --- |
| Buffer allocation | With `no_alloc`, creates a **size-0 dummy buffer** and sets every tensor's `buffer` to it | Always `ggml_backend_alloc_ctx_tensors_from_buft` — **real allocation** |
| `memory_breakdown()` | With `no_alloc`, `ggml_backend_alloc_ctx_tensors_from_buft_size(ctx, buft)` — **expected size including alignment** | Always `ggml_backend_buffer_get_size(buf)` |

So while the plan is being built, the recurrent storage is **actually allocated on the card.** `stage_memory_plan.cpp` then
reads the reduced `free` and compares it with a `required` that includes the cost just allocated.

The plans of the five 09-09 35B runs show exactly this arithmetic (card initial headroom 22.76 GiB).

| arm | n_seq | `CUDA0 RS buffer` | plan `free` | free + RS | `required` | Verdict |
| --- | ---: | ---: | ---: | ---: | ---: | :-- |
| 2 stage | 96 | 2,814 MiB | 20.01 | **22.76** | 14.01 | ✓ |
| 2 stage | 256 | 7,504 MiB | 15.43 | **22.76** | 19.72 | ✗ |
| 2 stage (retry) | 256 | 7,504 MiB | 15.43 | **22.76** | 19.72 | ✗ |
| 4 stage | 96 | 1,407 MiB | 21.38 | **22.75** | 6.96 | ✓ |
| 4 stage | 256 | 3,752 MiB | 19.09 | **22.75** | 10.14 | ✓ |

**The cause is not double-counting of weights but the recurrent allocation made for planning.** The GPU `context` term holds both the already allocated
RS and the attention KV, and the two are handled differently, so `model + compute + 2 × context` is not the exact condition either.
The 19.72 GiB actually needed for r256 fits on a 24 GiB card.

## Fix

Two places in upstream `llama-memory-recurrent.cpp` were aligned so that it answers the same way as `llama_kv_cache`.
This is the new compat patch `0026-noalloc-recurrent-residency.patch` (`layer: upstream_fix`).

- Allocation loop: with `hparams.no_alloc`, create a size-0 dummy buffer and set every tensor's `buffer`.
  Allocation and initialization semantics in run mode are unchanged.
- `memory_breakdown()`: with `no_alloc`, report the **expected size including alignment and padding** via
  `ggml_backend_alloc_ctx_tensors_from_buft_size`. The actual-size reporting path is unchanged.

**The fit check was not removed, and no correction adding context to `free` was made.** Configurations that genuinely lack space
must still be rejected; this fix only keeps the plan a plan.

The fix sits inside the compat boundary of the adopted upstream, and no private llama types move up to higher layers.

## Verification so far

| Check | Result |
| --- | --- |
| Applying 26 patches | `git apply --check` all passed |
| model-agnostic boundary | `validateCompatibilityPatch` passed (no model or architecture knowledge) |
| Prepared tree diff hash | `patch_set_sha256` = `f37f181c9d38afed04993921b30470dd69d8cfd5d232d05405f384a01439e738`, recomputed and verified |
| `patched_tree` | `7d66751252406ad0e952409355c5cc0a34530c55` |
| Pipeline ABI symbol | `llama_linkcpp_runtime_configure` confirmed present |
| **Compile** | CPU Release build, `llama.dll` **214/214 linked successfully** |
| Pinned upstream checkout | `git status --porcelain` empty — all work done in separate worktrees |

Before the fix, `patch_set_sha256` was `961bd89cd1197cef0d683f22d99d9451e25aba5eba5f360dac6aaa792b993d81`.
The reproduction procedure (apply 25 patches to a worktree at the pinned pin → `git diff --binary --full-index`) first reproduced that value
exactly, confirming the procedure itself was correct, and then the 26th patch was added on top.

## Not done yet — this is the next stage

It compiles, but **the behavior has not been verified.** All of the following remain, and they need a CUDA build and the remote 3090×2.

1. Confirm that the real RS allocation for planning is gone, distinguishing it from backend initialization cost.
2. Compare per-host/device `model`, `context` and `compute` plans with actual allocation at resident 96, 160 and 256.
3. No regression on attention, recurrent and hybrid, and on host/device paths.
4. Whether **configurations that genuinely lack space are still rejected** — that only the false rejection was fixed and no over-approval was introduced.
5. After the fix, a separate pass of real r256 load, peak memory, inference and UNLOAD.

**"Fixed the false rejection" and "r256 runs safely" are different completion conditions.**
The combined reservation when several processes share the same card cannot be replaced by individual stages passing fit either.
And **a successful r256 load is not service approval for raising resident** — resident evaluation under heavy continuous waves
comes after the B2/B3 acceptance and return budgets are wired in.

On build cost: recurrent is compiled as C++ into `llama.dll`, so during development a CPU build was enough for incremental verification,
as above. Real-hardware verification needs refreshed CUDA outputs, and at deployment the hash of the actual
DLL next to the server is checked as well.
