# Gate 5 pass-through capability validation

> 문서 지위 (2026-09-06): **날짜·환경 한정 증거**. 본문 날짜/커밋/모델/토폴로지의 관측이다. 현재 구현이나 다른 분산 환경의 완료 증거가 아니다.
> 현재 목표·상태·순서는 [실행 로드맵](../../../../../../../docs/distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](../../../../../../../docs/document-map.md)를 따른다.

Observed 2026-08-18 KST. This gate used the staged artifact
`.cache/staged-server-cuda-real-20260818/Release/p4_staged_server.exe`, rebuilt
from the existing prepared tree after the staged Windows synthetic-argv count
fix in `staged/server/src/server/plan.cpp`. No file under `upstream` was
changed. The real model was
`S:\models\linker-test\Qwen2.5-1.5B-Instruct-Q8_0.gguf`.

## Fast plan-parser results

The executable was started with a length-prefixed stdin plan and
`--validate-plan`; each process exited with code 0 unless noted below.

| Option | Result | Evidence |
| --- | --- | --- |
| `--top-k 1 --temp 0 --seed 1` | PASS: parser accepted | `PLAN_APPLIED`; `normal_decode_execution=1` |
| `--grammar "root ::= \"hello\""` | PASS: parser accepted | same capability report |
| `--logit-bias 21927+10` | PASS: parser accepted | same capability report |
| `--reasoning-budget 0 --reasoning-budget-message done` | PASS: parser accepted | same capability report |
| `--ctx-checkpoints 2 --checkpoint-min-step 0` | PASS: parser accepted | same capability report |
| `-ot "blk.0.*=CPU"` | PASS: parser accepted | same capability report |
| `--kv-unified` | PASS: parser accepted after staged fix | synthetic plan argv count is kept distinct from the real process argv |

The parser report for accepted normal plans was:

```text
mtp_parser=1;mtp_execution=0;speculative_parser=1;speculative_execution=0;
normal_decode_execution=1;mtp_requested=0;speculative_requested=0;
execution_blocker=none
```

The `--kv-unified` parser result is not an execution claim. It only proves that
the staged plan reaches `common_params_parse` without the Windows `--bind`
collision.

## Existing real execution evidence

The existing ignored Rust real-server test was run against the CPU staged
server and Qwen2.5 1.5B GGUF with `--temp 0 --seed 1`. The baseline HOP
returned token `21927` (`" Hello"`), grammar `root ::= "A"` returned token
`32` (`"A"`), and `--logit-bias 198+1000` returned token `198` (`"\n"`).
These option runs reached real prefill/decode; the test then failed at its
separate KV assertion with `invalid KV metadata`, so they are HOP-level
results, not full test passes. The prior independent compile regression
passed next-token equality after KV restore:

```text
KV_RESTORE_NEXT_TOKEN_EQUIVALENT token=21927 position=8
prefill_max=6 restored_max=6
```

This proves ordinary staged sampling and KV persistence for the existing
normal decode path. It does not prove that grammar, logit bias, reasoning
budget, checkpoint reuse, unified KV, or tensor override changes the generated
result.

## Capability disposition

* Sampler path: **normal sampling execution PASS**.
* Grammar: **HOP execution PASS** (`token=32`, text `A`); full lifecycle test
  **FAIL** only at subsequent KV metadata.
* Logit bias: **HOP execution PASS** (`token=198`, text newline under
  `+1000` bias); full lifecycle test **FAIL** only at subsequent KV metadata.
* Reasoning budget/forced completion: **parser PASS, execution NOT CLAIMED**.
  The staged runtime calls `common_sampler_init` directly but does not run the
  upstream server chat/reasoning control path that prepares reasoning tags and
  invokes forced completion.
* Context checkpoint: **parser PASS, execution NOT CLAIMED**. Checkpoint
  creation/reuse is implemented in upstream `tools/server/server-context.cpp`,
  not in the staged HOP runtime.
* Unified KV: **execution PASS within the tested scope**. Qwen2.5 1.5B CUDA
  2-stage `--kv-unified` restore→decode passed with byte/checksum and next-token
  equivalence. Other architectures and speculative combinations remain
  unverified; see `2026-08-18-kv-unified-forwarding.md`.
* `-ot`: **parser PASS, ownership effect NOT CLAIMED**. The staged loader's
  layer ownership decision and tensor override precedence still require a
  load-log or tensor-residency assertion.

MTP/speculative remains explicitly `parser=1, execution=0`; it was not enabled
or retested as part of this gate.

## Request-level `Sequence.options` result

The former adapter gap is closed for the raw opaque value. Staged v2 now has an
optional length-delimited options field, selected by flag `0x10`; the adapter
copies `Sequence.options` into every local HOP sequence, and the C++ server
preserves the value in the HOP result without interpreting sampler JSON. HELLO
advertises this raw-preservation capability as `request_options=1`.

Evidence:

* Rust library tests: **30 passed**, including v2 round-trip and adapter-copy
  assertions.
* C++ protocol test: **PASS** (`p4_staged_protocol_test.exe`, exit 0).
* C++ server option-preservation tests T6/T7: **PASS** in the instrumented
  server test binary. The complete server test remains non-green because its
  pre-existing durable-transaction T5 path exits with `0xc0000409`; this is
  not claimed as an options failure.
* Real Qwen2.5 1.5B Q8_0 HOP, CPU staged server, `S:\models\linker-test\Qwen2.5-1.5B-Instruct-Q8_0.gguf`:
  **PASS** for prefill HOP → options returned unchanged → UNLOAD, 15.65 s.
  HELLO included `request_options=1`; the HOP had 2 output descriptors and
  the test printed `REAL_E2E REQUEST_OPTIONS_PREFILL_ONLY round_trip=ok`.

The wire field is opaque across the adapter boundary, but the C++ tail now
interprets a deliberately strict semantic subset: `temperature`/`temp`,
`top_k`, `top_p`, `seed`, user `grammar`, and upstream-compatible `logit_bias`.
Per-sequence sampler state is recreated when the options string changes. HELLO
reports this as `request_options_semantics=temperature,top_k,top_p,seed,grammar,logit_bias`.
The Qwen2.5 1.5B dedicated test changed token `21927` to token `198` under a
request-level logit bias. This does not claim automatic support for every
`common_params_sampling` field or reasoning control. New options-bearing v2
frames require a reader that understands flag `0x10`; readers still decode old
v2/legacy payloads with an empty options value. Legacy direct payload encoding
does not carry the optional field.

## Post-change regression

After the request-options, native-checkpoint, and MTP ownership slices, the
CUDA C++ Release rebuild and CTest completed successfully:
`100% tests passed, 0 tests failed out of 8`.
The Rust workspace regression also completed successfully with
`cargo test --workspace --all-targets`; the staged adapter request-options
suite reported 30 passed. These are compile/contract regressions and do not
upgrade the opaque request-options field to semantic per-sequence sampler
support.
