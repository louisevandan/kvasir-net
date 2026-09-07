# 5000-token prefill / 5000 max-generation calibration

> 문서 지위 (2026-09-06): **날짜·환경 한정 증거**. 본문 날짜/커밋/모델/토폴로지의 관측이다. 현재 구현이나 다른 분산 환경의 완료 증거가 아니다.
> 현재 목표·상태·순서는 [실행 로드맵](../../../../../../../docs/distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](../../../../../../../docs/document-map.md)를 따른다.

## Conclusion

## Latest telemetry correction (2026-08-18)

The earlier conclusion below predates the keep-loaded drive and cumulative
telemetry fixes. A real 4-request x 8-token non-MTP regression now completed
with the same local two-stage model loaded once:

```text
completed=4 failed=0
observed_total_lines=16
logical_prefill_tokens=20000
logical_generation_tokens=32
aggregate logical prefill TPS=3893.537448
aggregate logical generation TPS=6.229660
average session prefill compute TPS=7730.407361
average session generation compute TPS=62.360238
4080 peak=5550 MiB / 9000 MiB
```

The cumulative-counter bug was that drive retained the first total observed
from a status snapshot instead of replacing it with the newest cumulative
value. That is now covered by a regression test and this run. The result is a
short telemetry/admission gate only; it does not accept 5k/5k sustained load
or semantic output quality. Evidence:
`target/real-two-stage-5000/20260818-telemetry-cumulative-4x8-v3`.

A later no-unload local run kept the two stage agents resident while two
requests each completed 5000 prompt and 5000 generation tokens:

```text
completed=2 failed=0
logical_prefill_tokens=10000
logical_generation_tokens=10000
average_session_prefill_tps=7447.761446
average_session_generation_tps=72.756882
queue_peak=1
4080 peak=6609 MiB / 9000 MiB
P4_DRIVE_KEEP_LOADED ... unload=skipped
```

This closes the local no-unload long-context and measurement gate, not semantic
quality or remote four-node long-run acceptance. Evidence:
`target/real-two-stage-5000/20260818-keep-loaded-5kx5k-2req-p2-final`.

The tensor-byte placement lower bound fits the requested local pair. A later
one-shot local run completed 5000 prompt tokens and 5000 generated tokens, but
the requested no-unload sustained run was not started in this audit. The
already-built
`p4-drive.exe` does not contain the source's `P4_DRIVE_KEEP_LOADED` capability,
and the existing four-node launcher has an unconditional cleanup path. Starting
the load with those artifacts would therefore eventually unload or terminate
the model, violating the request.

The later one-shot result is recorded at
`target/real-two-stage-5000/20260818-meaningful-seed-5kx5k/result.json`:
`completed=1`, `failed=0`, `prompt_tokens=5000`, `generation_tokens=5000`,
`context_size=20000`, `native_slots=2`, and `keep_loaded=false`. It proves
single-request capacity only. Its output was highly repetitive, so semantic
quality is not accepted. It does not prove sustained ingress, no-unload
reuse, or optimal parallelism.

The new guard/runner is
`benchmarks/2026-08-18-5000-token-prefill-generation.ps1`. It is preflight-only
unless an exact tokenizer binary, a rebuilt keep-loaded drive artifact, and an
explicit no-unload launcher are supplied.

## Live preflight

Observed 2026-08-18 KST:

```text
model = S:\models\unsloth\Qwen3.8-27B-GGUF\Qwen3.8-27B-Q6_K.gguf
staged artifact = .cache\staged-server-cuda-real-20260818\Release\p4_staged_server.exe
3090 tensor window = [0,49), 14.5691 GiB, budget 23 GiB
4080 tensor window = [49,64), 6.7334 GiB including the boundary, budget 11 GiB
```

The planner explicitly excludes KV cache, compute buffers, allocator
fragmentation, CUDA context, and runtime overhead. `n_ctx=10000` is the minimum
request plan for 5000 prompt tokens plus 5000 generated positions; it is not a
proof that the remaining 3090/4080 headroom is sufficient.

The CUDA staged Release directory is real and contains `p4_staged_server.exe`,
`ggml-cuda.dll`, `llama.dll`, and CUDA 13.1 cuBLAS DLLs. The runner contains
two `--validate-plan` calls; these are the safe no-load gate and do not send
`UNLOAD`. The audit executed the read-only tensor planner. The first runner
invocation exposed and then corrected a PowerShell parse error before any
model load; no model load/inference was started.

The corrected runner then completed the no-load gate at
`target\benchmark-5000-token\20260818131250`:

```text
PREFLIGHT plan_validation=passed unload=not-sent
PREFLIGHT tokenizer=False exact_prompt=False keep_loaded=False
PREFLIGHT blocker=no llama-tokenize binary; prompt bytes cannot prove 5000 model tokens
```

No `p4-agent`, `p4-drive`, or `p4_staged_server` process remained after the
preflight.

At the time of the check, no local P4 agent/drive/staged-server listener was
using ports 52000-52004 or 53001-53002. `nvidia-smi` reported GPU 0 (RTX 3090)
at 0 MiB and GPU 1 (RTX 4080) at approximately 950 MiB.

## Exact prompt-token contract

`P4_DRIVE_PROMPT_FILE` reports bytes only. It does not establish token count.
The staged runtime tokenizes the stage-zero prompt with the loaded model's
`common_tokenize(vocab, prompt, true, true)`, so calibration must use the same
GGUF vocabulary, BOS behavior, and special-token parsing. The runner therefore
requires `llama-tokenize --show-count` built from the same llama.cpp generation,
or an equivalent staged-server tokenizer probe, and rejects a prompt unless the
observed count is exactly 5000.

The generation request must remain separate:

```text
TOKENS=5000
P4_DRIVE_QUIET_MS=1800000
P4_DRIVE_OPTIONS={"temperature":0,"seed":1,"ignore_eos":true}
P4_DRIVE_KEEP_LOADED=1
```

`ignore_eos` is optional for a normal max-generation benchmark; it is shown when
the desired measurement is exactly 5000 decode steps rather than an EOS-bounded
completion. The authoritative completed-token count remains terminal metadata,
not streamed frame count.

## Parallel sweep plan

The safe order is one load followed by one inference shape, with no unload:

```text
prompt = 5000 tokens; max_generation = 5000
parallel = 1, then 2, then 4 (only if the same loaded plan advertises that ceiling)
collect = completed/failed/unanswered, terminal token counts, first failure,
          per-stage progress, queue/in-flight/credit peaks, native stderr,
          per-GPU memory/utilization, and process IDs
```

The current driver performs load and unload per invocation, so it cannot safely
run this sweep under the no-unload rule. A proper sweep needs one run-owned
launcher that loads once, runs all requested parallel points against the same
loaded fleet, and leaves cleanup to the operator. Separate process-per-point
runs are not safe on 23/11 GiB because their model residency would accumulate.

## Four-node remote integration

The existing SSH topology was proven only with the small Qwen2.5 1.5B model:

```text
central 3090 [0,7) -> central 4080 [7,14) -> remote 3090 [14,21) -> remote 3090 [21,28)
```

It uses ports 52003/52004 locally, SSH-forwarded 53001/53002 remotely, and a
reverse-forwarded return channel. That evidence proves four-node load/HOP,
concurrent short requests, and overlap; it does not prove the Qwen3.8 27B
large-context path. Its launcher also issues `UNLOAD` and kills owned processes
in `finally`, so it is not an admissible runner for this request.

For the requested Qwen3.8 27B plan, a new four-node plan would need layer
windows and model paths for all four nodes, e.g. a 3090/4080 local pair plus
two remote 3090s, with the 64-layer split re-planned for four windows. The
remote host must expose the same staged server/CUDA DLL hashes and the same GGUF
model identity. This was not started because the no-unload launcher and exact
remote model/artifact preconditions are missing.

## Reproduction / blocker command

Safe preflight, no model load:

```powershell
& F:\dev\linkcpp_product\benchmarks\2026-08-18-5000-token-prefill-generation.ps1
```

Exact calibration and preflight after building a matching tokenizer helper:

```powershell
& F:\dev\linkcpp_product\benchmarks\2026-08-18-5000-token-prefill-generation.ps1 `
  -TokenizerBinary 'F:\path\to\matching\llama-tokenize.exe' `
  -GeneratePrompt
```

The current drive artifact is blocked by the following observed fact:

```text
target\p4-release-validation-remediation27\release\p4-drive.exe
does not contain P4_DRIVE_KEEP_LOADED or unload=skipped
```

Do not substitute prompt bytes for the 5000-token count, do not run the current
four-node launcher for this target, and do not infer large-model decode success
from the existing tensor-byte or short Qwen2.5 four-node evidence.
