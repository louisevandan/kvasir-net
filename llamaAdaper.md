# llama.cpp Adapter 구현 요약

이 문서는 이 세션에서 확인·구현한 P4의 llama.cpp 어댑터와 staged runtime의 현재 상태를 요약한다. `MTP`는 현재 테스트 범위에서 제외한다.

## 구조

```text
OUTER
  -> p4-drive / p4-agent
    -> node
      -> llamacpp adapter
        -> staged C++ server
          -> patched llama.cpp runtime
```

주요 경로:

| 경로 | 책임 |
| --- | --- |
| [`layers/adapters/llamacpp/staged/adapter`](layers/adapters/llamacpp/staged/adapter) | Rust `Adapter` 구현, plan 전달, 서버 생명주기, 로컬 프로토콜 클라이언트 |
| [`layers/adapters/llamacpp/staged/server`](layers/adapters/llamacpp/staged/server) | C++ stage server, llama.cpp 호출, hop 입출력, KV 상태 처리 |
| [`layers/adapters/llamacpp/staged/compat`](layers/adapters/llamacpp/staged/compat) | 공식 llama.cpp에 적용하는 versioned compatibility patch set |
| [`tools/scripts/e2e/run-ssh-forwarded-real-four-node.ps1`](tools/scripts/e2e/run-ssh-forwarded-real-four-node.ps1) | 중앙 3090·4080과 원격 3090×2를 연결하는 실제 4노드 검증 실행기 |
| [`docs/protocol.md`](docs/protocol.md) | OUTER/P4가 이미 정의한 hop, sequence, option, capability 계약 |
| [`docs/buildplan.md`](docs/buildplan.md) | staged 구현 및 검증 게이트 |

## 구현된 동작

### 서버 생명주기

- 각 llama.cpp stage는 하나의 독립 프로세스로 실행된다.
- Rust adapter가 stage server를 생성·연결하고 프로세스 핸들을 소유한다.
- 정상 언로드는 `UNLOAD` 후 컨텍스트·모델·runtime을 정리한다.
- 비정상 부모 종료에 대비해 서버의 stdin은 기동 plan 뒤에도 닫지 않는다. stdin EOF는 liveness 종료 신호다.
- 원격 Windows의 `S:` 네트워크 드라이브는 비대화형 SSH 세션에 보이지 않으므로, 원격 interactive 계정의 숨김 Scheduled Task로 Agent를 실행한다.

### 분산 추론

- 모델 plan은 stage별 `layer_begin/layer_end`, KV 범위, batch, ubatch, context, GPU layer 수를 포함한다.
- 각 stage는 자기 레이어와 KV 범위만 소유한다.
- hop은 P4 protocol의 여러 sequence/window 계약을 따르며, staged adapter는 각 sequence의 cut-set payload를 stage server 사이에 전달한다.
- stage 출력은 `output_get`과 `terminal_get`을 모두 처리한 뒤 한 번 synchronize하고 전송한다.
- alias descriptor는 별도 payload를 보내지 않고 유효한 alias 범위만 허용한다.
- adapter Rust 코드에는 backend FFI를 두지 않고 C++ server가 llama.cpp 헤더와 라이브러리를 직접 사용한다.

### 모델 옵션 전달

- llama.cpp의 개별 옵션을 Rust adapter가 열거하지 않는다.
- OUTER/P4가 전달한 opaque option/plan을 adapter가 보존하고 C++ server가 llama.cpp `common` 계층으로 해석한다.
- 세밀한 offload, unified KV, sampling, reasoning budget, context/batch 옵션은 pass-through 대상이다.
- speculative decoding과 MTP는 staged 의미 검증이 필요한 별도 capability이며, 현재 MTP는 테스트 대상에서 제외한다.

## KV cache

- KV cache는 각 stage server가 소유한다.
- KV 저장/복원은 stage 범위, sequence, model identity, cache key, checksum을 대조한다.
- 복원 대상과 메타데이터가 맞지 않으면 복원하지 않는다.
- 현재 검수에서 `runtime/build identity`, context parameter, KV format, token position을 cache key에 명시적으로 포함해야 한다는 결함이 확인됐다. 이 부분은 최종 운영 승인 전 보강 대상이다.

## 실제 검증 결과

### 성공한 범위

- 중앙 RTX 3090·RTX 4080과 원격 RTX 3090×2를 SSH forwarding으로 연결했다.
- 모델 파일을 로컬/원격 디스크로 복사하지 않고 `S:\models\...` 공유 경로에서 사용했다.
- 원격 Agent를 숨김 Scheduled Task로 기동하고 각 원격 포트가 열리는 것을 확인했다.
- 4080 stage는 초기 실험에서 `n-gpu-layers=2`로 제한했다.
- 실행기는 산출물 SHA-256 검증, VRAM 샘플링, stage range/GPU layer override, hidden process, cleanup을 지원한다.

### MiniMax-M3 결과

사용 모델:

```text
S:\models\unsloth\MiniMax-M3-GGUF\MiniMax-M3-UD-Q5_K_S-00001-of-00008.gguf
```

시도한 stage 배치:

```text
ranges:   0:15,15:30,30:45,45:60
gpu:      4,2,4,4
4080 max: 10500 MiB
```

4개 stage 모두 모델 로더 진입까지 갔지만 다음 오류로 중단됐다.

```text
key not found in model: minimax-m3.attention.indexer.head_count
```

이는 분산 cut-set이나 원격 연결 실패가 아니다. 현재 pinned llama.cpp의 MSA 경로가 요구하는 인덱서 메타데이터가 `S:`의 초기 MiniMax-M3 GGUF에 없기 때문이다. 공식 모델 문서도 해당 GGUF를 실험적 PR #24523 기반 포맷으로 설명한다.

따라서 아직 다음 결과는 없다:

- MiniMax-M3 4노드 실제 토큰 생성
- 5,000-token prefill
- 5,000-token generation
- prefill/generation TPS
- 병렬 calibration 및 queue saturation

결과 파일:

- [`target/ssh-forwarded-four-node-e2e/m3-smoke-20260819-3/result.json`](../target/ssh-forwarded-four-node-e2e/m3-smoke-20260819-3/result.json)
- [`target/ssh-forwarded-four-node-e2e/m3-smoke-20260819-3/central-agent-52003.err.log`](../target/ssh-forwarded-four-node-e2e/m3-smoke-20260819-3/central-agent-52003.err.log)

## 현재 진행 중인 M3 호환 작업

공식 upstream은 수정하지 않고 별도 prepared tree에서 legacy M3 GGUF dense-attention compatibility를 실험 중이다.

- 초기 M3 GGUF의 누락된 MSA metadata에는 안전한 기본값을 사용한다.
- indexer tensor는 optional로 생성한다.
- `--flash-attn 0` dense path에서는 indexer tensor를 사용하지 않도록 한다.
- 이 호환층이 로딩을 통과하는지 먼저 확인한 뒤, 그 산출물에 staged ABI를 연결한다.

이 방식은 MSA 정확성이나 sparse-attention 성능을 증명하지 않는다. 목적은 현재 보유한 초기 GGUF를 사용해 staged 분산 로딩/hidden-state 전달 자체를 검증하는 것이다.

## 알려진 문제와 다음 게이트

1. legacy M3 compatibility source가 컴파일되는지 확인한다.
2. 단일 stage에서 M3 모델 load와 1-token decode를 확인한다.
3. 중앙 3090·4080 두 stage에서 load와 hop을 확인한다.
4. 원격 3090×2를 포함한 4-stage 1-token inference를 확인한다.
5. 4080 VRAM을 11GB 이하로 유지하면서 stage range/GPU layer를 조정한다.
6. 5k/5k 의미 있는 프롬프트 테스트를 실행한다. 이때 `batch-size=5000`, 충분한 context, MTP 제외를 명시한다.
7. 동일한 parallel 수로 동시 요청을 만들고 이후 요청을 계속 투입해 node queue/adaptor queue가 쌓이는지 측정한다.
8. prefill TPS, generation TPS, 통합 TPS, 세션별 평균 TPS를 별도로 기록한다.
9. 실제 결과가 통과한 뒤에만 `buildplan.md`와 validation evidence를 갱신한다.

## 주의사항

- `apps/p4/layers/adapters/llamacpp/upstream`은 교체 가능한 공식 llama.cpp 경계이므로 Linker 전용 코드를 넣지 않는다.
- compatibility 수정은 versioned patch/prepared tree에 둔다.
- M3 로딩 오류를 레이어 배치 오류로 해석하지 않는다. 모델 포맷과 runtime 지원 수준을 먼저 맞춘다.
- 실험 실행은 모두 hidden/background 방식으로 수행하고, 중단 시 중앙/원격 Agent·SSH tunnel·VRAM sampler를 함께 정리한다.
