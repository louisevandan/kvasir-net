# MiniMax M3 MSA distributed load rejection

2026-09-13. Status: **RED — LOAD/SESSION and inference for a valid MSA GGUF split into stages are not supported.**

## Test setup

- Model: `bartowski/MiniMax-M3-GGUF`, `MiniMax-M3-Q5_K_S`, 8 shards,
  295,228,545,984 bytes, including the MSA indexer metadata/tensors
- Execution: `--flash-attn on --no-kv-unified`, context 4,096, sequence 1,
  batch 128, ubatch 64, f16 KV
- Devices: only the central RTX 3090 was used; the RTX 4080 was excluded. The middle stages used Spark CUDA
  unified memory and Mac21 Metal. Only the expert weights of the central discrete stage were
  placed in RAM, for the capacity shortfall.
- Contiguous layer cuts: central CUDA0 `0..14`, Spark CUDA0 `14..38`, Mac21 MTL0
  `38..50`, central CUDA0 `50..60`

The raw run data is in `target/minimax-m3-msa-20260913/smoke/`. This directory is
local hardware output and is not committed as repository evidence.

## Verdict

With the same setup, native `--inspect-memory-plan` was rejected with exit 7 on every stage: CUDA, Spark CUDA unified memory
and Metal. The common first cause was:

```text
llama_init_from_model: failed to initialize the context:
llama.cpp memory implementation does not declare stage-local residency support
llama.cpp failed to create the no-alloc context plan
```

To isolate whether this was a defect of plan mode only, a real CREATE/LOAD was run once with a new node id and generation.
All 4 nodes were created, but the Mac21 stage rejected LOAD with
`stage runtime initialization failed`, native exit 5. There were
0 SESSIONs, and no query was submitted. Normal responses and prefill/generated TPS are therefore both
unmeasured. They are not compared with the TPS of the old dense-fallback run without the indexer.

## Code cleanup

The compat patch `0029-minimax-m3-msa-stage-residency.patch` only added a static flag to the MSA wrapper.
The actual acceptance check is the virtual call `llama_memory_i::supports_linkcpp_stage_residency()`,
so that flag did not change the wrapper's acceptance state, and the support claim in the comment was also wrong.
The incomplete opt-in and the resulting support wording were removed, returning to the 27-patch fail-closed state.
The adapter's rejections of a missing indexer, disabled flash attention, and multiple sequences combined with unified KV
are kept.

After the failure, it was confirmed that no native child process remained. The central, Spark and Mac21 agents that took part in the test
were restarted, and all three endpoints were confirmed to be listening again, which reclaimed the failed node ledger.
M3 stage residency implementation and re-testing are not continued in this version.
