# 35B across four GPUs on two machines, sixty sessions

Observed 2026-08-20 KST. The reference measurement of a service shape: sixty
requests of a 5,000-token prompt against a 5,000-token answer, ten admitted at
once, arriving thirty together and then five every thirty seconds.

## Topology

```text
central 4080  CUDA 0  layers [0,6)    agent :52003   stage-0
central 3090  CUDA 1  layers [6,17)   agent :52004   stage-1
remote  3090  CUDA 0  layers [17,29)  agent :53001   stage-2
remote  3090  CUDA 1  layers [29,40)  agent :53002   tail-3
```

Model `S:\models\unsloth\Ornith-1.0-35B-GGUF\Ornith-1.0-35B-UD-Q5_K_S.gguf`
(`qwen35moe`, 40 layers). Layers are dealt in the ratio of the cards' usable
memory, 11:23:23:23 GiB. Flash attention on, KV cache `q8_0`, `--no-mmap`, no
CPU offload — the whole model fits in VRAM at this split.

Measured peak VRAM: central 3090 7,864 MiB, central 4080 **7,433 MiB** against
a guard of 11,000, remote 3090s 8,716 MiB each.

## Result

Sixty of sixty completed, none failed, and all four driver verdicts held.

| | over the run | per session |
| --- | ---: | ---: |
| prefill | 19.18 tok/s | 1,996.5 tok/s |
| generation | 18.20 tok/s | 18.37 tok/s |
| combined | 37.38 tok/s | — |

Latency p50 5,722 s, p95 13,327 s, max 15,482 s over 15,662 s of wall clock.

Prefill counted 300,350 tokens — sixty prompts of exactly 5,000 plus the
five-token index that makes each request its own. Generation counted 285,105
rather than 300,000 because sequences that reached their end token stopped
there; the shortest produced 207 tokens and the longest the full 5,000.

Per-session spread: prefill 491 to 3,927 tok/s, generation 10.67 to 27.20.

## What the numbers say

Ten sessions at 18.37 tok/s each aggregate to 18.20 tok/s. Concurrency is not
becoming throughput.

The reason is one number: **a hop carries one sequence**. Counted two ways —
35,000 decode laps against 35,500 graph executions on the first stage, and
every retained runtime sample reporting one sequence per hop. So the device
reads this stage's weights once per token per session, and a stage spends most
of its time on the hop rather than on the model.

The same deployment prefills at 1,996 tok/s per session. Same cards, same
layers; the only difference is how many tokens are in the call. That ratio is
the size of what batching a decode lap is worth.

## What this run also settled

The frame limit. A prefill hop carries one F32 cut per token per sequence: a
5,000-token prompt at n_embd 2,048 is 39 MiB for one sequence, so a ten-wide
prefill window is 391 MiB. Both the outer P4 frame and the staged local wire
capped a body at 128 MiB, which refused every window past three sequences —
measured as 53 of 60 requests failing on `HOP envelope too large` while the
deployment itself was healthy. Both limits are now two gibibytes, which is
what let this run complete.

## Reproduction

```powershell
apps\p4\tools\scripts\e2e\run-ssh-forwarded-real-four-node.ps1 `
  -LocalModel  'S:\models\unsloth\Ornith-1.0-35B-GGUF\Ornith-1.0-35B-UD-Q5_K_S.gguf' `
  -RemoteModel 'S:\models\unsloth\Ornith-1.0-35B-GGUF\Ornith-1.0-35B-UD-Q5_K_S.gguf' `
  -PromptFile  '.cache\ornith-35b-5000-exact.txt' `
  -Requests 60 -Tokens 5000 -PromptTokens 5000 -Parallel 10 -ArriveMilliseconds 0 `
  -InitialBurst 30 -BatchRequests 5 -BatchIntervalMilliseconds 30000 -VaryPrompts on `
  -ContextSize 150000 -BatchSize 512 -UBatchSize 512 -FlashAttention 1 `
  -StageRanges '0:6,6:17,17:29,29:40' -GpuLayers '6,11,12,11' `
  -Max4080VramMiB 11000 -LocalGpuDevices '0,1' -QuietMilliseconds 1800000
```

The prompt is exactly 5,000 model tokens, trimmed from a real one by
`scripts/validation/tokenizer/llama-token-count.cpp` against the model's own
vocabulary. Evidence — every prompt and every complete answer — is written to
`sessions-evidence.md` in the run directory when `P4_DRIVE_EVIDENCE_FILE` is
set.

## The remote host needs a logon

The model lives on the NAS, and `S:` is a mapping that belongs to an
interactive logon. The remote agents start as a hidden Scheduled Task under
that logon, which is why they can open it. After a reboot with nobody logged
in, the task does not run at all and the run fails at readiness — observed
once, and it is not a fault in the deployment.
