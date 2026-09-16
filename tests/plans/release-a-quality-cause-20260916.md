# Test Plan: Release A I0 answer-cause telemetry

## Created

2026-09-16 KST. This is a diagnostic of the existing I0 RED, not an I0 acceptance rerun.

## Goal

Distinguish observable source-record retrieval, integer calculation, task composition, and output-format
failures for the medium and long Qwen3.5-122B-A10B cases. Keep the original strict oracle and failed
distributed/standalone responses unchanged. Do not infer hidden model reasoning from token output.

## Environment

- Spark GB10 only, same readable Qwen3.5-122B-A10B UD-Q5_K_S GGUF and standalone llama-server binary
  used by the prior reference control. This Windows PC only creates inputs and judges outputs.
- One server load, sequential requests, context 131072, parallel 1, batch 128, ubatch 64, flash on,
  F16 KV, no speculative decoding. Greedy settings: temperature 0, top-p 1, top-k 0, min-p 0,
  repeat penalty 1, seed 20260916, max output 2048, `cache_prompt=false`.
- Source records, selected facts, and the exact original response are from sealed corpus-v2 case-04
  (32000 tokens) and case-06 (100038 tokens). The prior original responses serve as unchanged controls.

## Preconditions

1. Verify HEAD/worktree, prior corpus and response hashes, model shard size/hash, server binary hash,
   empty task GPU/process/port, and the protected agent PID/port. Verify the remote tokenizer and its
   libraries before probe materialization. Abort before load if any identity differs.
2. Materialize all six diagnostic prompts and expected JSON from source facts before any request.
   Bind every prompt and oracle SHA-256 plus exact tokenizer IDs/counts in one immutable manifest.
3. The local judge must pass baseline and independent missing, prompt-swap, token-count, non-EOS,
   malformed-JSON, wrong-fact, wrong-calculation, and format mutations before model load.

## Steps

For each of case-04 and case-06, run exactly these three probes in this order on the same load:

| Probe | Input | Required visible telemetry |
| --- | --- | --- |
| E: source extraction | Entire original record block; request only selected IDs' revision/current/resistance/hours/pressure | Exact source-derived facts, raw JSON, prompt identity and token count |
| C: calculation control | Only the three exact selected source records; original integer formula and output schema | Exact power, energy, pressure flag and temperature flag |
| T: combined trace | Entire original record block; request facts and calculated output in separate JSON fields | Exact facts and exact calculations in one response; format and EOS |

Retain full `/completion` responses, generation settings, stop reason, evaluated/predicted tokens,
timings, start/end wall times, input hashes, binary/model identities and server log. One malformed or
missing response is a diagnostic failure; it never becomes I0 GREEN. After all probes, terminate only
the exact task-owned server PID and confirm its port/GPU process are gone while protected agents live.

## Expected Results

- E wrong facts proves retrieval fails for the extraction probe. C wrong math proves calculation fails
  even without a long context. T correct facts with wrong math proves a calculation failure in the
  combined task. T wrong facts proves retrieval failure in the combined task. T completely correct
  while the unchanged original was wrong identifies original-task instruction sensitivity.
- Multiple failures may coexist. If diagnostic prompts are correct while the original remains wrong,
  report the remaining uncertainty instead of claiming a hidden mechanism. The service-level cause
  of the existing silent wrong answer is separately tested: there is no deterministic check between
  model output and the exact source-derived oracle before returning a normal completion.
- No probe changes I0's expected answer, H0 seal or next distributed LOAD authority.

## Logs To Capture

Manifest, six prompts/oracles/token ID hashes, six raw responses, judge classification, server log,
process/port/GPU cleanup, and the unchanged original response hashes. Record exact setup failures and
the final number of executed probes. Do not run a revised probe in the same diagnostic arm.
