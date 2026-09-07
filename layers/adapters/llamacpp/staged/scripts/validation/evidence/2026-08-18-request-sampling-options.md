# Request-level sampling option audit

> 문서 지위 (2026-09-06): **날짜·환경 한정 증거**. 본문 날짜/커밋/모델/토폴로지의 관측이다. 현재 구현이나 다른 분산 환경의 완료 증거가 아니다.
> 현재 목표·상태·순서는 [실행 로드맵](../../../../../../../docs/distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](../../../../../../../docs/document-map.md)를 따른다.

Audited 2026-08-18 against pinned compat revision
`3e3a7a416d6588597523c792fc5874a543d59ed9`. The upstream checkout was not
modified.

## Existing boundary

The startup plan continues to use `common_params_parse()` and
`common_params_sampling` directly. Request JSON is parsed only by the staged
`apply_request_options()` helper when a tail-stage sampler is first created;
changing the JSON invalidates and recreates that sequence's sampler. No
second llama.cpp command-line parser was added.

Already supported before this slice:

```text
temperature, top_k, top_p, seed, grammar, logit_bias,
reasoning_budget_tokens, reasoning_budget_start_tag,
reasoning_budget_end_tag(s), reasoning_budget_message
```

## Implemented option group

The staged request surface now forwards and validates these
`common_params_sampling` fields:

```text
min_keep
typical_p / typ_p
n_prev, n_probs, sampler_seq, min_p, top_n_sigma, dynatemp_range,
dynatemp_exponent, adaptive_target, adaptive_decay, ignore_eos
penalty_last_n / repeat_last_n
penalty_repeat / repeat_penalty
penalty_freq / frequency_penalty
penalty_present / presence_penalty
dry_multiplier, dry_base, dry_allowed_length, dry_penalty_last_n,
dry_sequence_breakers
xtc_probability, xtc_threshold
mirostat, mirostat_tau, mirostat_eta
grammar_lazy, grammar_triggers, preserved_tokens, generation_prompt
```

Validation rejects non-finite numbers, out-of-range probabilities, invalid
Mirostat modes, negative counts, and malformed DRY breaker arrays before
sampler construction. The values are consumed by upstream
`common_sampler_init()`, so penalties, MIN_P, DRY, XTC, typical-P, and Mirostat
retain upstream sampler ordering and state semantics.

The HELLO capability string now lists the expanded request option surface.

## Tests

`request_options_test.cpp` now checks the complete option group against a
loaded model, verifies the parsed values, and verifies that `mirostat=3` is
rejected. Its existing real-model assertions still cover request-level token
effect through a forced logit bias and reasoning-budget behavior.

The target was rebuilt from the existing staged CMake configuration with the
explicit Windows CMake executable and ran against the real Qwen2.5 1.5B GGUF:

```text
REQUEST_OPTIONS_EXTENDED_PARSE_OK
REQUEST_OPTIONS_GRAMMAR_TRIGGER_PARSE_OK preserved_token=0
REQUEST_OPTIONS_TOKEN_DIFF baseline=21927 forced=198 forced_text=
REQUEST_OPTIONS_REASONING_BUDGET0 forced=522 text=</ expected_first_end_tag=522
```

The extended parser test also rejects invalid `mirostat=3` before sampler
construction. This proves parsing and sampler construction for the listed
group, plus the existing real-model logit-bias and reasoning-budget effects;
it does not prove a distinct output change for every scalar option.

## Deliberately not exposed

Sampler-chain replacement, backend sampling, and speculative/MTP controls remain
outside request JSON. Grammar trigger and preserved-token fields are now parsed
and passed to the upstream sampler; their model-level generation effect is not
claimed by this test. Backend sampling remains a startup/device capability.
