# Distributed load of the old MiniMax M3 dense-fallback GGUF

2026-09-13. Status: **4-stage load, single inference, and capacity 1 and capacity 4 waves passed for the old conversion**.
This document records the compatibility fix for a MiniMax M3 conversion that lacks MSA information, and the
real load and inference on two physical Windows hosts. The capacity 4 result is acceptance evidence for a short fixed
workload; long-context and service performance acceptance is not yet granted.

## 2026-09-13 MSA re-audit and verdict correction

This run does not accept correct sparse-attention execution of MiniMax M3. Re-inspecting the full headers of the eight
GGUF shards used showed 0 `minimax-m3.attention.indexer.*` metadata entries and 0 `indexer` tensors.
So this is not an artifact on which MSA can be turned on by an adapter option. Injecting only metadata would not restore
the semantics, because the required indexer projection/norm weights are missing.

The MSA execution conditions in current llama.cpp were audited as well. Even a well-formed MSA GGUF falls back to dense
attention when flash attention is off, or when unified KV is used with `n_seq_max > 1`.
The capacity 4 run below used `--flash-attn on --kv-unified`, so the same plan cannot be reused even after swapping in a
well-formed MSA GGUF. The staged server's non-unified KV path falls back safely to per-request HOP execution, so the first
acceptance configuration is `--flash-attn on --no-kv-unified`. Merging several sequences into one native decode call on
non-unified KV is an optimization that needs separate implementation and verification.

The first shard of `bartowski/MiniMax-M3-GGUF` Q5_K_S, checked for comparison, contains indexer head 4,
key length 128, top-k 16, block size 128, local block 1 and 28 actual indexer tensors. CUDA and Metal distribution was
re-judged with the eight shards of the same quantization, but MSA memory did not accept stage-local residency, so LOAD
rejected it. The product adapter keeps rejecting a missing indexer, disabled flash attention, and the combination of
multiple sequences with unified KV. The on-hardware result for well-formed MSA is recorded in a
[separate rejection record](2026-09-13-minimax-m3-msa-distributed-rejection.md).

## Cause and fix

The model is `unsloth/MiniMax-M3-GGUF`, `MiniMax-M3-UD-Q5_K_S`, 8 shards,
298,756,339,264 bytes (278.239 GiB), 60 layers, 128 experts / 4 active.
The existing `434ddbbc0` native exited during first-stage initialization with
`GGML_ASSERT(hparams.indexer_block_size > 0)`. This GGUF is dense M3, so
`has_msa=false` and it has no indexer block size. `0015-official-minimax-m3-dense-gguf.patch`
changed that assert to run only inside the `has_msa` branch. This fix was a compatibility measure that made the old
conversion runnable at the time; after the re-audit above, the product acceptance path rejects it.

The fix commit is `f4b0feb62`. The 26 patches were reapplied in order to pristine `434ddbbc0`, and
manifest validation passed 7/7. The final identity is as follows.

| Item | Value |
| --- | --- |
| upstream | `434ddbbc0e30522e897670681e503b797c12b7c1` |
| patch set | `ebf53e855f11e5a848ccddf34c8cc517056f839d91148b702c04f4eb4c218379` |
| patched tree | `73b67197837f10811bb2560ec93324886c18360a` |
| Windows CUDA server SHA256 | `b4e2f59965ffebb6bc98af798e1cc30ebb2cabbfb4b946854310f64d3584673f` |

## Real load

The run generation is `1789245852580` and the session is `minimax-m3-1789245852580`.
It used the two RTX 3090 cards in M42 server2 and the RTX 4080 + RTX 3090 in the central host.
Each stage owns 15 layers; the expert FFNs of the owned layers are placed in CPU RAM, and
the remaining layer bodies and 4K KV on the assigned CUDA device. batch/ubatch is 128/64,
and sequence capacity is 1.

| stage | Host / device | layer / KV range | Result |
| --- | --- | --- | --- |
| 0 | M42 / CUDA0 | `[0,15)` | loaded, session ready |
| 1 | M42 / CUDA1 | `[15,30)` | loaded, session ready |
| 2 | central / CUDA0 | `[30,45)` | loaded, session ready |
| 3 | central / CUDA1 | `[45,60)` | loaded, session ready |

All four `LOADED` responses had the same upstream, patch set and stage wire ABI.
All four `SESSION_READY` responses were received too. The total took 1,653.6 s, mostly spent reading about 70 GiB of
expert weights per stage from the NAS. Right after completion, the central host had 65.1 GiB RAM
free and 4,731 / 10,784 MiB GPU memory used, and M42 had about 105.7 GiB RAM free and
4,839 / 4,839 MiB GPU memory used. GPU utilization samples taken at that moment are not a performance metric.

Spark, the Ubuntu laptop, the two Mac minis and TUF had only agents and 0 native stages.
The two MI250 machines had no P4 processes either. This result is therefore CUDA load evidence for two physical hosts and
four GPUs, not load evidence for the whole LAN cluster or for Metal/ROCm.

## Single output and capacity 1 wave

A Korean-language facility calculation prompt, with MiniMax M3's raw chat template and a thinking-disabled prefix applied,
was submitted to the same resident session. The single request processed 157 prefill tokens and
125 generated tokens up to EOS, with `completed=1`, `released=1`, `passed=true`. TTFT was
187.043 s, logical prefill was 0.839 token/s, and the generation span was 141.834 s, about 0.88 token/s.
The response computed `17 L/min × 13 min = 221 L` and distinguished the constant-flow assumption from the two field
measurements. The 229-character response contained no UTF-8 replacement character.

Next, a fixed workload was run on the same capacity 1 session: 2 requests immediately and 2 more after 60 s.
All four requests completed with EOS and were released, and they satisfied the 84, 138, 42 and 162 L calculations and the
per-request explanations. UTF-8 replacement characters were 0 in all four responses.

| Item | Value |
| --- | --- |
| Completed / released / errors | 4 / 4 / none |
| wall / generated tokens | 334.387 s / 426 |
| aggregate generated TPS | 1.274 token/s |
| Total prefill tokens | 525 |
| physical batch | 436 (prefill 10 / decode 426 / mixed 0) |
| Physical batch width | mean 2.181, max 64, UBATCH 64 mean fill 3.408% |
| Observed max ready sequences / open batches | 1 / 0 |

This result is evidence that the backlog was processed sequentially through a capacity 1 slot and that the slot was
reused four times. There were no concurrent sequences, so it does not prove in-flight batch saturation or a performance
effect from raising capacity. To check that, the same workload was run again at sequence capacity 4.

## Capacity 4 concurrent wave

Sequence capacity and total KV context were raised from 1/4K to 4/16K, keeping the same four prompts,
arrival times (2 immediately, 2 after 60 s), max tokens, sampling, batch/ubatch 128/64 and layer
cut. On M42, the same eight shards were copied to a local SSD so as not to depend on the NAS login session.
Each file length, the total of 298,756,339,264 bytes and the transfer exit codes
match. A separate SHA256 comparison that rereads all 278 GiB was not done within the time limit, but the
native loader read all eight shards and loaded four stages with the same model metadata.

Because the agents on both hosts advertise loopback addresses, the existing helper ports 42003 (M42) and
42004 (central) were connected through SSH local/reverse tunnels. Port 52005 on the central host falls inside the Windows
excluded dynamic port range 51952--52151 and could not be bound. The agent binary SHA256 was
`c551db0c77d93cff1d9f87ba2b00622bca6627cc6b67b2c6c6d84507e9dc234b` on both hosts.

The first capacity 4 load passed through four `LOADED` and four `SESSION_READY` responses, but about 100.5 s after the wave
started, the M42 stage 1 native process exited. The Windows Application
Error recorded fault module `nvptxJitCompiler64.dll`, exception `0xc0000005`, offset
`0x5853a`, WER report ID `d4f85b29-b61d-4e80-826d-8ca9b6bf1875`.
The native log just before showed `CUDA graph warmup reset` repeatedly. This run received only the response fragment `**`
with `completed=0` and `released=0`, so it is a failure and not a performance sample.

In direct response to this failure, `GGML_CUDA_DISABLE_GRAPHS=1` was applied to all four stages with the other
conditions unchanged, and the model was reloaded as generation `1789254973468`, session
`minimax-m3-batch-1789254973468`. All four `LOADED` and four
`SESSION_READY` responses arrived, and the load took 1,458.3 s. The same wave run next
completed all four requests to EOS and released all four slots. The four responses correctly computed 84, 138,
42 and 162 L, and UTF-8 replacement characters were 0 in all of them.

| Item | capacity 1 | capacity 4 + CUDA graph off | Change |
| --- | ---: | ---: | ---: |
| Completed / released | 4 / 4 | 4 / 4 | same |
| wall | 334.387 s | 151.745 s | -54.6% |
| generated tokens | 426 | 407 | differs by sampling outcome |
| aggregate generated TPS | 1.274 | 2.682 | +110.5% |
| Total prefill tokens | 525 | 525 | same |
| physical batch | 436 | 416 | -20 |
| prefill / decode / mixed batch | 10 / 426 / 0 | 9 / 406 / 1 | 1 mixed |
| Batch width mean / max | 2.181 / 64 | 2.240 / 64 | mean +2.7% |
| UBATCH mean fill | 3.408% | 3.501% | +0.093%p |
| Observed max ready sequences / open batches | 1 / 0 | 3 / 3 | concurrent progress confirmed |

Per-request TTFT was 16.155, 87.572, 109.185 and 221.790 s at capacity 1, and 18.295, 26.356, 22.760 and 22.231 s
in the capacity 4 run. The first request was 2.140 s slower, but the next
three were faster by 61.216, 86.425 and 199.559 s respectively. This is evidence that the long head-of-line
wait at capacity 1 was removed and that several sequences actually progressed.

The throughput gain is between arms that changed capacity 4 and CUDA graph disabling together.
This run therefore cannot separate the causal effect of raising capacity alone or the cost of CUDA graphs.
Also, the mean physical batch width stayed almost the same and p50/p90 were both 1 row, so UBATCH saturation
did not occur. What this result accepts is normal completion of four concurrent requests, reduced tail TTFT, and
increased aggregate TPS under that combined condition. Long-prompt mixes, sustained arrival, GPU utilization,
service latency distribution and the optimal capacity are separate gates.

## Failure boundaries and remaining verification

The first pre-fix run failed on the dense metadata assert. The first remote run after the fix exited with `0xc0000135`
because the new deployment directory lacked the CUDA runtime DLLs; this was resolved by adding `cublas64_13.dll`,
`cublasLt64_13.dll` and `cudart64_13.dll` to the deployment manifest.

The final capacity 4 loader state was `created=4`, `loaded=4`, `session_ready=4`,
`passed=true`. After the wave, all four native stages stayed resident. The two GPUs on M42 each used
4,974 MiB, and idle utilization samples were 0%.

The first harness attempt while preparing the capacity 1 verification reused the outer
channel/generation of the previous load connection and failed immediately with
`event stream ended mid-frame`. Native stage executions were 0, and it was excluded from the performance
samples. The run above with a new channel/generation is the accepted sample.

The local originals are `full-load`, `single-inference`,
`sequential-wave`, `batch-load`, `batch-wave` and `deployment-manifest.json` under `target/minimax-m3-load-20260913/`.
`target/` is not durable evidence across checkouts, so this document also records the identity,
generation, topology and boundaries needed for the verdict.

## Verification commands

- compat manifest: valid, 26 patches
- patch classification: valid, upstream_fix 4 / stage_hook 18 / model_feature 4
- CTest on the same Windows CUDA Release build: 16/16 passed
- `npm run docs-lint`: 96 files clean
- `node tools/scripts/docs-lint.mjs --all`: fails because it counts the 122 ignored upstream documents in the local
  `.cache/llama-pipeline-upstream` as unregistered repository documents. This is kept distinct from the tracked-document
  gate, and this result is not reported as clean.
