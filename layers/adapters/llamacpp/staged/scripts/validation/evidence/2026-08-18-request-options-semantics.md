# Request-level sampler semantics evidence

> 문서 지위 (2026-09-06): **날짜·환경 한정 증거**. 본문 날짜/커밋/모델/토폴로지의 관측이다. 현재 구현이나 다른 분산 환경의 완료 증거가 아니다.
> 현재 목표·상태·순서는 [실행 로드맵](../../../../../../../docs/distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](../../../../../../../docs/document-map.md)를 따른다.

Scope: staged C++ runtime/server only. No Rust adapter, staged wire protocol,
`protocol.md`, or upstream source was modified for this slice.

## Implemented surface

`Sequence.options` is parsed only when a tail-stage decode creates or replaces
the sequence sampler. Supported fields are:

* `temperature` (and the llama.cpp alias `temp`)
* `top_k`
* `top_p`
* `seed`
* `grammar` (user GBNF string)
* `logit_bias` (upstream-compatible array or object form)
* `reasoning_budget_tokens`
* `reasoning_budget_start_tag`
* `reasoning_budget_end_tags` and `reasoning_budget_end_tag`
* `reasoning_budget_message`

`request_tag` is accepted as non-semantic metadata. Other keys are rejected
with an explicit unsupported-option error. Sampler state is replaced when the
same sequence receives a different options string, so one sequence cannot
silently retain another request's sampler.

## Tests

Build:

```text
cmake --build .cache/staged-server-llama --config Release --target p4_staged_request_options_test
PASS (exit 0)
```

Real execution:

```text
model=S:\models\linker-test\Qwen2.5-1.5B-Instruct-Q8_0.gguf
server=.cache/staged-server-llama/Release/p4_staged_request_options_test.exe
REQUEST_OPTIONS_TOKEN_DIFF baseline=21927 forced=198 forced_text=
REQUEST_OPTIONS_TEST_EXIT=0
```

The baseline and request-level sampler used the same prompt and model. The
request options were:

```json
{"logit_bias":{"198":1000},"temperature":0,"top_k":1,"top_p":1,"seed":1}
```

Token 198 is the model's newline token. The test initially exposed an error:
object keys were being passed through the string-tokenization path, so
`{"198":1000}` did not mean token 198. The parser now follows llama.cpp's
server schema: a numeric object key is interpreted as a token id; non-numeric
string keys are tokenized as text. The corrected real run returned token 198.

Reasoning-budget execution was also verified with the same Qwen2.5 1.5B GGUF.
The dedicated test uses budget `0`, start tag `" Hello"` (the first generated
token), and end tag `"</think>"`. The first decode emits the start token and
the following decode is forced to the first end-tag token:

```text
REQUEST_OPTIONS_REASONING_BUDGET0 forced=522 text=</ expected_first_end_tag=522
REQUEST_OPTIONS_TEST_EXIT=0
```

The runtime creates the request sampler during tail prefill so it exists
before decode; `common_sampler_init` then constructs the upstream
reasoning-budget sampler from tokenized tags and forced sequence. The message
field is tokenized and added before the first end tag, but a non-empty message
was not separately asserted in this run.

## Nonclaims

This proves the request-level sampler path, logit-bias effect, and budget-zero
forced end-tag execution. It does not claim separate model-level effect tests
for every scalar field or non-empty reasoning message behavior. MTP,
speculative decoding, and arbitrary sampler JSON remain unsupported. The
existing full staged Rust decode path remains a separate `llama_decode` status
`-3` issue when it forwards the prefill cut-set back into a full-range tail;
this dedicated test clears that cut-set as the runtime KV regression test does.
