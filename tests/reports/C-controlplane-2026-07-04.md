# Test Report — control plane (C1–C6)

- Date: 2026-07-04
- Stack: `docker-compose.control.yml` = controller (GPU-less) + node1 (RTX 4080) +
  node2 (RTX 3090) node-agents. UI at http://localhost:9000.

## API / driver verification (curl)

| Area | Result |
|------|--------|
| C2 registry/binding | both nodes register & bind (exclusive `bound_to` = controller id); node-agent `/info` reports GPU + budgets (4080 14/32/8, 3090 22/48/16) |
| C5 model manager | `/api/models` lists OLMoE, Qwen2.5-32B, qwen0.5B with sizes |
| C1 plan (via API) | OLMoE feasible on 1 node; 32B feasible split; 32B par64 → INFEASIBLE + reason |
| C3 serve (single) | OLMoE: plan→start worker→GPU-less master, `ready:true`; chat "Red, Yellow, Blue…" |
| C3 serve (distributed) | 32B: workers [node1,node2], **VRAM 4080=15.3GB + 3090=7.7GB** (split across both GPUs, GPU-less controller master); chat: coherent sentence on distributed inference |
| C4 gateway OpenAI | `/v1/chat/completions` correct; `/v1/models` lists served model |
| C4 gateway Responses | `/v1/responses` → `response` object, `output_text` "Paris…" (stateless) |
| C4 gateway Anthropic | `/anthropic/v1/messages` → `message` object, content "Tokyo…", usage |

## Web UI acceptance tests (headless Playwright)

`bash scripts/acceptance.sh` — 6/6 passed (screenshot: tests/acceptance/ui.png):

| ID | Scenario | Result |
|----|----------|--------|
| AT-1 | nodes table shows RTX 4080 & RTX 3090 with VRAM/RAM/cores | PASS |
| AT-2 | models list populated | PASS |
| AT-3 | plan → FEASIBLE verdict + placement | PASS |
| AT-3b | absurd parallel → INFEASIBLE + suggested knobs | PASS |
| AT-4 | serving status shows `running` | PASS |
| AT-5 | chat → real completion end-to-end through the cluster | PASS |

## Normalization (post-review fixes)

The first controller pass blocked on a synchronous serve (HTTP/UI hung for minutes while
the GPU-less master read the model over the slow FUSE mount) and reported `running` while
the master was still loading (503s). Fixed and re-verified:

| Fix | Result |
|-----|--------|
| async non-blocking serve (phase loading→running\|error) | `/api/serve` returns in ~2s; status shows `staging model` → `starting workers` → `loading model` → `running` |
| fast model staging (FUSE → `/stage` volume, cached) | OLMoE serve 75s cold → **21s warm**; 32B 410s cold → **192s** after clean boot (stage cached) |
| phase reconciliation | a crashed master flips to `error`, no false `running`/503 |
| registry persistence + startup auto-rebind | after `docker restart`/clean `down`+`up`, both nodes auto-restore with no manual re-add |
| clean-boot reproducibility | fresh `up` → nodes restore → serve 32B → running → both GPUs (3090 7.7GB + 4080 15.3GB) → chat "OK" → acceptance 6/6 |

## Conclusion

The full control plane works end-to-end: a GPU-less controller recognizes nodes (each
contributing a declared budget), reads the model architecture, plans a feasible placement
(layers/KV/`-ot`) or rejects with a reason, launches the GPU-less master + RPC workers,
serves distributed inference across both GPUs, and exposes OpenAI / Responses / Anthropic
APIs and a web UI verified by headless acceptance tests.
