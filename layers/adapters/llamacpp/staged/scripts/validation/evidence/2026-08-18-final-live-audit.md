# Final live non-MTP audit

> 문서 지위 (2026-09-06): **날짜·환경 한정 증거**. 본문 날짜/커밋/모델/토폴로지의 관측이다. 현재 구현이나 다른 분산 환경의 완료 증거가 아니다.
> 현재 목표·상태·순서는 [실행 로드맵](../../../../../../../docs/distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](../../../../../../../docs/document-map.md)를 따른다.

## Runtime evidence

The current CUDA stage server and Release agent/drive were exercised without
changing the pinned `upstream` tree.

- Model: `S:\models\Qwen2.5-1.5B-Instruct-Q8_0.gguf`
- Topology: central RTX 3090 + central RTX 4080 + remote RTX 3090 x2
- Existing ports only: driver `52000`, agents `52003/52004`, SSH forwards
  `53001/53002`
- Prompt: 5,000 tokens
- Generation: 5,000 tokens per request, non-MTP, `ignore_eos=true`
- Requests/parallel: 4 / 4
- `batch=512`, `ubatch=512`, context `40000`

Result directory:
`target/ssh-forwarded-four-node-e2e/20260818-final-audit-4x5k5k`

The runner recorded `passed=true`, `completed=4`, `failed=0`,
`unanswered=0`, `tokens=20000`, `elapsed_ms=475168`,
`peak_node_queue=3`, and `peak_in_adapter=3`. Logical aggregate metrics were
20,000 prefill tokens and 20,000 generation tokens, each 42.09 tok/s over the
run; average session prefill was 5,319.31 tok/s and generation was 37.43 tok/s.
No process or listener remained on the five ports after cleanup.

## Single-process and staged equality evidence

- `cargo test -p p4-llamacpp-staged-adapter --test against_real_llama -- --ignored`
  passed with HOP/decode token `21927`, KV save `230200` bytes, restore/drop,
  and UNLOAD.
- `logits_equality --ignored` passed with 151,936 values,
  `max_abs=0`, `max_relative=0`; 2-stage and 4-stage sampled outputs matched
  for 8 decode steps.
- `two_real_stages --ignored` passed for 2 sequences and 10 prefill tokens.
- `multi_stage_kv_e2e --ignored` passed for both 2-stage and 4-stage layouts,
  including per-stage checksum equality and KV_DROP file removal.

The real tests intentionally report `mtp_execution=0` and
`speculative_execution=0`; this audit is non-MTP only.
