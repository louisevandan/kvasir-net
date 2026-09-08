# 모델 적재 카탈로그

> 문서 지위 (2026-09-08): **OUTER 운영 자료**. 모델 적재 파라미터와 실측 메모리 기록이다.
> P4는 계획 문자열을 해석하지 않고 전달만 한다. 계층 책임은
> [격리 계약](../../../docs/layer-isolation-contract.md), 현재 목표·순서는
> [실행 로드맵](../../../docs/distributed-batching-roadmap.md)이 소유한다.

모델 적재 파라미터는 P4의 지식이 아니라 OUTER가 공급하는 값이다. 그러나 어떤 모델을 어떤
분할·오프로딩으로 올릴 수 있고 그때 메모리의 어느 요소가 얼마를 차지하는지는 실측해야만 알 수 있다.
현대 모델의 층 구성은 단순한 공식으로 계산되지 않는다. 같은 파라미터 수라도 hybrid attention,
recurrent state, MoE 라우팅, NextN 블록이 섞이면 KV·compute·model 버퍼가 서로 다르게 늘어난다.
이 폴더는 그 실측을 모델별로 남겨, 이후 P4를 제어하는 웹 UI가 계산 대신 조회하도록 한다.

## 무엇이 기록되는가

`models/<model-id>.json` 하나가 한 논리 모델이다.

| 절 | 내용 |
| --- | --- |
| `identity` | 발행자·저장소·shard 파일 목록·첫 shard 경로·바이트 수 |
| `shape` | 아키텍처, block/trunk 층 수, NextN 층, expert 수와 활성 수, embedding 폭, 학습 context, KV head/key/value 길이, full-attention 간격, expert·비expert 바이트 |
| `prompt` | EOS/BOS 토큰 id와 GGUF chat template에서 검출한 표식 |
| `runs[]` | 실행 1회 = (전략, context, stage 수, seq 수). 각 stage의 device/host별 `model`·`context`·`compute` 바이트와 그 합, 그리고 stage 서버가 출력한 버퍼 줄 원문 |
| `runs[].totals` | 같은 실행의 stage 전체 합계. 두 stage를 동시에 올릴 때 실제로 필요한 host RAM은 이 값이다 |
| `loaded_contexts` | 실제 적재(`mode: "load"`)로 증명된 context 목록 |

`runs[].stages[].measured`가 `actual`이면 실제 가중치를 적재한 뒤의 측정값이고, `plan`이면
가중치를 읽지 않는 계획 패스의 값이다. 둘을 함께 얻은 실행은 `plan_equals_actual`로 대조 결과를 남긴다.

## 왜 계획 패스를 신뢰할 수 있는가

`--inspect-memory-plan`은 `no_alloc` 모델로 컨텍스트만 만들어 메모리 내역을 계산한다. 가중치를 읽지
않으므로 200 GiB 모델도 몇 초면 끝난다. 2026-09-08까지 이 경로는 tail stage의 host compute를 과소
보고했고([증거](../../../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-08-noalloc-plan-underestimate.md)),
`0025-noalloc-reserve-size-max.patch`로 고쳤다. 그래서 계획값을 카탈로그의 기본 측정으로 쓰되,
모델마다 실제 적재를 함께 돌려 `plan_equals_actual`로 확인한다.

## 재현

```bash
node test/benchmarks/model-catalog/inventory.mjs --root "S:\\models"
node test/benchmarks/model-catalog/build-jobs.mjs --mode plan --contexts 4096,32768,102400,262144 --out target/model-catalog/plan-jobs.json
node test/benchmarks/model-catalog/remote-probe.mjs run --jobs target/model-catalog/plan-jobs.json --tag plan1
node test/benchmarks/model-catalog/remote-probe.mjs status --tag plan1
node test/benchmarks/model-catalog/remote-probe.mjs fetch --tag plan1 --out target/model-catalog/plan1.jsonl
node test/benchmarks/model-catalog/render.mjs --results target/model-catalog/plan1.jsonl --jobs target/model-catalog/plan-jobs.json --out test/benchmarks/model-catalog/models
```

`inventory.mjs`는 모델 공유를 볼 수 있는 기기에서 실행한다. `remote-probe.mjs`는 SSH로 원격을
제어하지만 프로브 자체는 **대화형 예약 작업**으로 돌린다. 매핑 드라이브는 로그온 사용자의 세션만
보므로 SSH 명령이 직접 모델을 열 수 없기 때문이다. 하네스 agent가 쓰는 것과 같은 방식이다.

## 적재 전략

| 전략 | 내용 | 언제 |
| --- | --- | --- |
| `vram_only` | 소유한 층 전체를 장치에 둔다. 소유하지 않은 층만 CPU 패턴으로 제외 | stage당 가중치+KV+compute가 장치에 들어갈 때 |
| `expert_cpu` | MoE routed expert 가중치를 host에 두고 CPU에서 계산. 라우터·attention·norm·KV는 장치 | MoE 모델이 장치에 들어가지 않을 때 |
| `dense_ffn_cpu` | dense 모델의 FFN 가중치를 host로. attention은 장치 | dense 모델이 장치에 들어가지 않을 때 |

`vram_only`/`expert_cpu`/`dense_ffn_cpu` 분류는 옵션 이름이 아니라 stage 로그가 보고한 실제 버퍼
배치로 확정한다. 검증 규약 H0의 `resource_tier` 판정도 같은 원칙을 쓴다.

## 주의

- 공유 드라이브에서 `--no-mmap`을 쓴다. mmap은 SMB에서 페이지 폴트로 적재가 사실상 멈춘다.
- NextN(MTP) 블록이 있는 모델은 trunk 층 수가 `block_count - nextn_predict_layers`다. 이 값을 넘겨
  자르면 `llama-graph.cpp`의 층 창 단언에 걸려 stage가 죽는다.
- gemma-4는 13층부터 KV를 공유하므로 그 구간 안에서 stage를 자르지 않는다.
- `fits_current_free`는 **그 stage 하나**의 판정이다. 여러 stage를 동시에 올릴 때의 host RAM은
  `runs[].totals.host_required_bytes`로 판단한다.
- 이 폴더는 적재 가능성과 메모리 실측을 기록할 뿐, 응답 품질이나 처리량을 승인하지 않는다.
