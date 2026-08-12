# 모델 적재 계약

`MODEL_LOAD.stage_plan`은 256 KiB 이하의 JSON 객체다. P4 전송 계층은 내용을
해석하지 않으며, 선택된 어뎁터가 `load_options`의 공통 필드와
`adapter_options`의 구상 런타임 전용 필드를 검증·선별하여 적용한다.

## `load_options`

```json
{
  "flash_attention": "enabled",
  "mmap": false,
  "kv_cache": {
    "type_k": "q8_0",
    "type_v": "q8_0",
    "offload": true
  },
  "batching": {
    "strategy": "ready-queue-dynamic",
    "max_sequences": 64,
    "node_limits": [
      {
        "node_id": "local-4080",
        "max_sequences": 64,
        "calculation": {
          "method": "minimum",
          "terms": [
            { "name": "requested_parallel", "value": 100, "source": "controller request" },
            { "name": "measured_node_limit", "value": 64, "source": "4080 VRAM evidence" }
          ],
          "result": 64
        }
      },
      {
        "node_id": "local-3090",
        "max_sequences": 100,
        "calculation": {
          "method": "minimum",
          "terms": [
            { "name": "requested_parallel", "value": 100, "source": "controller request" },
            { "name": "measured_node_limit", "value": 100, "source": "3090 VRAM evidence" }
          ],
          "result": 100
        }
      }
    ],
    "context_batch_tokens": 100,
    "context_ubatch_tokens": 100,
    "calculation": {
      "method": "minimum",
      "terms": [
        { "name": "requested_parallel", "value": 100, "source": "controller request" },
        { "name": "context_batch_tokens", "value": 100, "source": "controller model-load option" },
        { "name": "context_ubatch_tokens", "value": 100, "source": "controller model-load option" },
        { "name": "verified_stage:local-4080", "value": 64, "source": "measured profile" },
        { "name": "verified_stage:local-3090", "value": 100, "source": "measured profile" }
      ],
      "result": 64
    }
  },
  "adapter_options": {}
}
```

각 `node_limits[]`의 `max_sequences`는 다음 식으로 독립 계산한다.

```text
min(requested_parallel, context_batch_tokens, context_ubatch_tokens,
    verified_limit(node))
```

최상위 `max_sequences`와 `calculation`은 노드별 설정을 이해하지 못하는 기존
어뎁터를 위한 호환성 fallback이며 모든 `node_limits`의 최솟값이다. 새 호스트
런타임은 `node_id`가 일치하는 값을 해당 프로세스에만 적용한다. 모든 런타임
노드는 정확히 한 번 나타나야 하며, 일치하는 실측 프로필이 없는 노드는 1로
제한한다.

현재 Ornith 1.0 35B의 100-slot 프로필은 4080을 64, 3090을 100으로 둔다.
4080은 100-slot 적재와 물리 배치 16에서 15,833/16,376 MiB가 관측됐고 과거
물리 배치 100이 실패했으므로 64를 검증 대상으로 선택했다. 같은 배치에서
3090은 최대 14,491/24,576 MiB로 10,085 MiB 이상 남았으므로 요청 병렬도 100을
상한으로 사용한다. 이는 최종 성능 최적점이 아니라 다음 실측에서 통과 여부를
판정할 모델 적재 정책이다.

`context_batch_tokens`와 `context_ubatch_tokens`는 llama.cpp의 토큰 창 설정이고
노드별 `max_sequences`는 해당 GPU cycle에 함께 처리할 ready session 수다.
어뎁터는 GPU가 유휴가 되는 시점에 자신의 FIFO에서
`min(ready_count, node.max_sequences)`개를 꺼내 동적 배치를 만든다. 이는
`parallel` 세션 수나 에이전트 메시지 동시성을 제한하지 않는다. P4 주 작업 큐는
이 계산이나 GPU 역압을 소유하지 않는다.

## 적용과 거부

- ControllerInstance는 `loadOptions`를 정식 `stage_plan.load_options`로 직렬화한다.
- 현재 native 어뎁터는 호스트 런타임 요청에 그대로 전달한다. 호스트는 스키마와
  계산 결과를 검증하고 flash attention, mmap, KV 형식/offload, batch/uBatch 및
  노드별 sequence 상한을 각 stage process에 적용한다.
- 미지원 옵션을 조용히 무시하면 안 된다. 이미 기동된 process를 가리키는 stock
  llama-server 어뎁터는 model-load 옵션을 적용할 수 없으므로 명시적 `ERROR`를
  반환한다.
- 다른 구상 런타임은 공통 필드 중 지원 항목과 `adapter_options`를 자체 정책으로
  선별하되, 실제 적용값을 binding detail 또는 런타임 관측값으로 보고해야 한다.

`load_options`는 `stage_plan` 내부 계약이므로 P4B1 v5의 frame layout은 변하지
않는다. 의미 계약의 소비자는 ControllerInstance, 각 어뎁터,
`llama_domain`의 호스트 런치 경계다.
