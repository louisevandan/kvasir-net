# P4 256세션 통신·추론 최적화 실측 보고서

> 문서 지위 (2026-09-06): **역사·구 계획**. 당시 계획/관측을 보존한다. 현재 상태·실행 순서·승격 기준으로 사용하지 않는다.
> 현재 목표·상태·순서는 [실행 로드맵](distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](document-map.md)를 따른다.

기준일: 2026-08-10. 이 문서는 P4(Proxy Pipeline Parallel Protocol)의
로컬 2-GPU Pipeline 실행을 실제 요청·응답 trace, stage 계측, 원시 GPU 샘플로
재구성한 근거 문서다. 구성·로그인·모델 적재만으로 성공을 주장하지 않는다.

## 결론

14:14 레이어 분할, 고정 backend sampler, 공유 메모리 경계에서 **서로 독립적인
controller/Pipeline 4개가 각 64세션을 동시에 처리하여 총 256/256 요청을 성공**했다.
그 뒤 terminal 역할을 RTX 4080으로 옮긴 reverse 14:14도 256/256을 통과했고,
terminal compute와 wall time을 더 낮췄다. 단일 native context의
`parallel=batch=ubatch=256`도 256/256 완료, 오류 0, 실제 생성 토큰 상한 위반 0,
TCP fallback 0으로 통과했다.

자연 EOG 종료가 비교를 왜곡하지 않도록 benchmark 전용 `benchmark_ignore_eog`를
추가했다. 이 모드의 reverse 14:14, **4 × 64 physical microbatch**는 요청당 정확히
500 completion tokens, 총 128,000 tokens를 37.985초에 처리해 **3,369.71 tok/s**를
기록했다. reverse 15:13 재균형도 256/256, 128,000 tokens, TCP fallback 0으로
통과했지만 37.827초, **3,383.82 tok/s**로 차이는 0.4%뿐이다. 레이어 한 장 이동은
현재 병목의 실질적 해법이 아니다.

공유 메모리 전송과 terminal 반환은 작고 TCP batch fallback은 계속 0회다. 4 × 64
고정 길이에서 4080 terminal GPU 평균 활성률은 약 80–82%이나 3090 first stage는
약 42–44%에 그친다. stage 0의 downstream wait는 stage 1 compute와 같은 규모다.
Nsight Systems로 같은 256세션을 다시 실행한 결과 terminal 4080의 CUDA graph 실행
union은 70.55%이고 p95 graph gap은 17.127 ms, stage 1의 평균 kernel queue time은
12.962 ms다. **P4 중계 병목은 아니지만 terminal CUDA 제출/동기화 경로에는 아직
유휴 구간이 남아 있다.** tensor-core instruction 점유는 이 측정만으로 알 수 없으므로
다음 판정은 terminal stage에 대한 Nsight Compute counter로 제한한다.

## 범위와 판정 규칙

| 항목 | 고정 조건 |
| --- | --- |
| 모델 | Qwen2.5-1.5B-Instruct-Q8_0, 로컬 CUDA 2-stage Pipeline |
| 요청 | 한국어 Rust 설명 요청, 요청별 생성 상한 500 |
| sampler | terminal backend: `temperature=0.2`, `top_p=0.9`, `top_k=20`, `seed=7` |
| 세션 | 4 × 64, 2 × 128, 1 × 256의 세 가지 physical microbatch 형태를 모두 실측 |
| 경계 | CUDA P2P/IPC 불가(`can_access:false` 양방향)이므로 host shared memory 사용 |
| 유효 trace | 프롬프트·최종문장 비어 있지 않음, `max_tokens=500`, accepted 1회, DONE 1회, `completed=true` |

4 × 64와 2 × 128은 동일 물리 GPU 위의 진짜 256 동시 세션 시스템 시험이지만,
각 lane이 별도 `llama_context`다. 1 × 256은 단일 context의 256 sequence를 검증한다.
따라서 context graph 상한과 scheduler의 실제 batch 폭을 구분해서 해석한다.

## 지금까지의 실험 이력

| 상태 | 실행/변경 | 결과 | 해석 |
| --- | --- | --- | --- |
| 통과 | TCP hidden-state 경계 | 256/128, 54,550 events, 96.749 s | 초기 통신 기준선 |
| 통과 | shared-memory 경계 | 256/128, 54,956 events, 80.402 s | host 공유 메모리로 경계 비용 축소 |
| 통과 | terminal batch return + persistent CPU sampler | 256/256, 56,357 events, 58.621 s | sampler worker/buffer 재사용으로 CPU sampler 비용 축소 |
| 통과 | CPU sampler, 4 × 64, 11:17 | 256/256, 14,189 events, slowest 8.793 s | 짧은 64-lane 기준선 |
| 통과 | backend sampler, 4 × 64, 11:17 | 256/256, 14,231 events, slowest 19.950 s | terminal 3090 평균 89.73%, first 4080 평균 28.79% |
| 거절 | backend sampler 물리 256/256 | context graph가 368 bytes 부족 | 정적 sampler graph 예산 누락 |
| 통과 | 새 graph 예산, 15:13, 64 | 64/64, 3,612 token events, 15.239 s | 예약식과 backend sampler를 실제 생성까지 검증 |
| 통과 | 새 graph 예산, 12:16/13:15/14:14/15:13 | 각 64/64, 오류 0 | 14:14를 256 후보로 선택 |
| 통과 | 새 graph 예산, 14:14, 4 × 64 | **256/256, 14,082 token events, wall 18.011 s** | 현재 기준 결과 |
| 거절 | 15:13, 4 × 64, 4080 first / 3090 terminal | 256/256이나 wall 26.029 s, terminal compute 64.495 s | 레이어 한 개 이동이 이 배치에서는 악화 |
| 통과 | reverse 14:14, 4 × 64 | **256/256, 14,151 token events, wall 16.527 s** | 3090 first / 4080 terminal로 현재 최선 |

생성 길이가 요청마다 다르므로 단순 wall time만으로 layer cut을 선택하지 않았다.
동일 조건에서 stage compute/token, downstream wait, trace 완결성을 함께 보았다.

## 정적 sampler graph 실패와 수정

15:13, 64세션에서 초기 예약식은 모델 stage가 실제로 보유한 tensor 수만 세고
전체 graph가 sampler 체인을 만들 때 필요한 context metadata를 충분히 세지 못했다.
실패는 Pipeline cut-set이 아니라 upstream `build_sampling()` 이전/도중의 context
metadata pool에서 일어났다.

| run | 시도 | 관측 |
| --- | --- | --- |
| `backend-sampler-15-64-v4` | stage metadata 조건식 | needed 1,273,296; available 1,272,928 |
| `...-v5` | sampler당 8개 추정 | needed 738,080; available 737,712 |
| `...-v6`~`v9` | Pipeline reserve 16→17→32, debug marker | 매번 368 bytes 차이; marker 전에 실패 |
| `...-v10` | stage base 2배 + 8/sequence | 47번째 sampler chain에서 metadata pool 소진 |

v10의 계측으로 첫 sampler chain은 graph node 37개, 후속 chain은 graph node 36개를
추가하며, metadata tensor object는 chain마다 37개가 필요함을 확인했다. 같은 `res`
값이 graph capacity와 tensor metadata pool 양쪽에 영향을 주므로 graph node 수만
맞추면 다시 실패할 수 있다.

현재 호환 계층은 stage-aware base를 2배로 잡고, 활성 sampler마다 **64개 + 안전
object 1개**를 예약한다. 64는 관측된 37 object보다 여유 있는 보수적 상한이며
VRAM/전송량이 아니라 host metadata의 소량 증가다. 변경은 공식 llama.cpp checkout
밖의 버전 고정 compatibility patch에만 존재한다.

| 검증 | 결과 |
| --- | --- |
| 호환 patch set | `b7f842e37ca8642bbaab86ebfe208cdda19642ed586abda7af899675e6dddc21` |
| upstream 준비 | `prepare-pipeline-upstream.mjs --json` 통과 |
| native pipeline stability | CUDA build 후 CTest `pipeline-stability` 5/5 통과 |
| 생성 검증 | `backend-sampler-15-64-v11`: 64/64 trace-valid, 오류 0 |

관련 소스는 [`0004-llama-context.patch`](../native/compat/3e3a7a416/0004-llama-context.patch)와
[`0006-llama-graph.patch`](../native/compat/3e3a7a416/0006-llama-graph.patch)다.
`apps/p4/layers/adapters/llamacpp/upstream`에는 Linker/P4 변경을 넣지 않았다.

## 64세션 layer-cut 대조

아래 GPU 평균은 실행마다 출력 길이가 다르므로 보조 지표다. 선택의 주 근거는
두 stage의 compute/token과 완료 지연이다.

| 분할 | 생성 token events | 완료 window | 4080 compute/token | 3090 compute/token | first downstream wait | 완료 p95 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 12:16 | 3,624 | 14.304 s | 1.623 ms | 1.997 ms | 7.454 s | 13.247 s |
| 13:15 | 3,660 | 15.501 s | 1.865 ms | 2.022 ms | 7.739 s | 14.733 s |
| **14:14** | **3,386** | **10.023 s** | **0.924 ms** | **1.723 ms** | **5.977 s** | **9.344 s** |
| 15:13 | 3,612 | 15.239 s | 2.005 ms | 1.874 ms | 7.068 s | 14.285 s |

14:14는 이 대조군에서 두 stage의 최대 compute/token과 완료 지연이 가장 낮았다.
다만 64세션 단독에서는 두 GPU 모두 포화되지 않았으므로, 이 선택은 256 동시
시험으로 다시 확인해야 했다.

## 최종 256 동시 세션 실측

실행은 서로 다른 P4 listen, Pipeline listen, native stage port를 사용한 네 lane을
같은 시점에 기동했다. 외부 PowerShell job은 마지막에 비정상 종료 코드를
보고했지만, 이는 job wrapper의 종료 상태다. P4 실행 자체는 네 lane 모두
`P4_PIPELINE_E2E_CLIENT_PASS`를 기록했고 아래 독립 artifact 검증이 이를 대체한다.

| lane | 완료/오류/무결성 위반 | token events | 요청 window | 완료 p95 | 4080 compute | 3090 compute |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| lane1 | 64 / 0 / 0 | 3,596 | 17.711 s | 15.581 s | 2.543 s | 13.214 s |
| lane2 | 64 / 0 / 0 | 3,372 | 17.693 s | 15.388 s | 2.515 s | 13.313 s |
| lane3 | 64 / 0 / 0 | 3,472 | 18.011 s | 15.035 s | 2.463 s | 13.348 s |
| lane4 | 64 / 0 / 0 | 3,642 | 16.973 s | 15.877 s | 2.341 s | 12.688 s |
| **합계/벽시계** | **256 / 0 / 0** | **14,082** | **18.011 s** | 전체 p95 15.667 s | 9.862 s 합 | 52.563 s 합 |

전체 trace에서 accepted 평균/95분위는 41/51 ms, 첫 token은 5.103/5.446 s,
완료는 10.606/15.667 s(최대 17.966 s)다. 최종 프롬프트 trace는
“Rust 언어를 한국어로 간단히 설명해. 핵심 특징을 한 문장으로 포함해.”를
보존한다. lane별 첫 응답의 최종 문장도 모두 실제 추론 결과로 보관되어 있다.

### 통신과 GPU 근거

| 지표 | 4080 first stage | 3090 terminal stage | 의미 |
| --- | ---: | ---: | --- |
| GPU 평균 사용률 | 27.809–28.078% | 86.297–87.044% | terminal만 거의 지속적으로 활성 |
| GPU p95 사용률 | 47% | 99–100% | first stage는 큰 여유가 남음 |
| first downstream wait 합 | 56.517 s | — | terminal 결과 대기와 일치 |
| stage compute 합 | 9.862 s | 52.563 s | terminal decode가 약 5.3배 큼 |
| TCP batch fallback | 0 | — | shared-memory/batched 경계에서 TCP 대기열 없음 |
| hidden transfer 속도 | 약 46–120 MB/s (lane별 표본) | — | payload 전달은 작은 compute 대비 지배적이지 않음 |

terminal 반환 send는 lane별 수십 ms, sampler apply도 수십 ms 수준이다. 반면
3090의 `decode_submit_us`는 lane별 약 11.7–13.0 s다. 따라서 현 시점에서 socket,
event loop, shared-memory 구조를 다시 바꾸는 것은 근거가 없다. 통신층은 256
동시 세션을 잃지 않고 중계했고, 다음 병목은 model placement와 terminal decode다.

## Reverse rank 14:14: terminal을 RTX 4080으로 이동

기존 planner는 4080을 first, 3090을 terminal rank로 선택했다. P4 plan 자체에는
stage별 `node`/`node_id`가 있으므로 wire를 바꾸지 않고 benchmark runner에
`P4_PIPELINE_STAGE_NODE_ORDER`를 추가했다. 이는 명시된 placement를 plan에 기록하는
실험용 입력이며, native launch는 기존 `ringProcessLaunches()`가 그 node mapping을
그대로 사용한다.

64세션 선행 run(`backend-sampler-reverse-14-64-v1`)은
`stage_nodes=[p4-gpu-3090,p4-gpu-4080]`, 64/64 완료, 오류 0, TCP fallback 0으로
mapping과 생성 경로를 먼저 검증했다. 이후 같은 매핑으로 4 × 64를 동시에 실행했다.

| 비교: 14:14, 4 × 64 | 기본 rank (4080 first → 3090 terminal) | reverse rank (3090 first → 4080 terminal) | 변화 |
| --- | ---: | ---: | ---: |
| 유효 완료 / 오류 / 무결성 위반 | 256 / 0 / 0 | 256 / 0 / 0 | 동일 |
| 생성 token events | 14,082 | 14,151 | 비슷한 길이 |
| wall time | 18.011 s | 16.527 s | -8.2% |
| first stage compute 합 | 9.862 s | 10.501 s | +6.5% |
| terminal stage compute 합 | 52.563 s | 40.972 s | -22.1% |
| first downstream wait 합 | 56.517 s | 43.228 s | -23.5% |
| terminal GPU 평균 / p95 | 86.3–87.0% / 99–100% | 73.2–78.7% / 94% | terminal 과점 완화 |
| first GPU 평균 / p95 | 27.8–28.1% / 47% | 30.1–30.7% / 47–63% | 아직 여유 큼 |
| TCP batch fallback | 0 | 0 | 통신 경로 유지 |

reverse rank는 stage compute 비율을 약 5.3:1에서 약 3.9:1로 줄였지만 아직 균형은
아니다. 15:13을 기본 rank로 옮긴 대조는 256/256을 통과했어도 token-normalized
terminal 비용과 wall time이 모두 악화됐으므로 채택하지 않는다. 다음 대조는
**reverse rank에서만** 15:13을 확인해 3090 first stage에 일을 더 주고 4080 terminal
부담을 실제로 더 낮출 수 있는지 판단한다.

### Reverse 15:13 거절과 다음 가설

reverse 15:13도 선행 64세션과 4 × 64의 256세션을 모두 trace-valid로 통과했다.
그러나 256세션에서 14,709 token events, wall 21.647 s, first/terminal compute 합
13.074/52.058 s, first downstream wait 55.410 s였다. reverse 14:14의
10.501/40.972 s, 43.228 s보다 모두 나쁘므로 이 cut은 거절한다.

원인은 통신이 아니라 4 × 64 실행 형태다. scheduler의 physical microbatch는
`min(batch, ubatch)`이고 각 독립 native context의 active session은 64개뿐이므로,
`batch=64`, `ubatch=64`에서 GPU에 전달되는 한 batch도 최대 64다. controller를
늘려도 서로 다른 native context의 batch는 합쳐지지 않는다. 따라서 다음 최소
실험은 **reverse 14:14, 128세션 한 lane**이다. 이를 통과하면 2 × 128 lane으로
256 동시 세션을 구성한다. 목적은 통신 구조를 바꾸지 않고 physical microbatch를
128로 키워 CUDA kernel shape와 두 stage의 유휴 시간을 개선하는 것이다.

### 2 × 128 physical microbatch 통과와 재측정

128세션 단일 lane(`backend-sampler-reverse-14-128-v1`)은 128/128 trace-valid,
`max_batch_size=128`, TCP fallback 0으로 선행 검증됐다. 두 lane을 처음 동시에
올린 시도는 inference 전에 실패했는데, 539xx/540xx가 Windows TCP excluded range
53851–54550에 포함됐기 때문이다. 보존한 native stderr는 정확히
`cannot listen on <port>`를 보였고 VRAM, graph, transport 실패가 아니었다. 해당
실패 group을 삭제하고 제외 범위 밖 531xx를 사용한 재시도만 성능 결과로 채택한다.

처음의 `r2` 결과는 P4 adapter가 `DONE.generated_tokens`에 text chunk 수를 기록하던
시점의 결과였다. 비교값의 의미를 고정하기 위해 adapter를 수정한 뒤 같은 조건을
`v4`로 재측정했다. `v4`의 completion token은 마지막 native SSE usage의
`completion_tokens`이며, usage가 없는 경우에만 text chunk 수로 fallback한다.

| 비교: reverse 14:14, 256세션 | 4 × 64 (batch 64) | 2 × 128 (batch 128, v4) | 변화 |
| --- | ---: | ---: | ---: |
| 유효 완료 / 오류 / 무결성 위반 | 256 / 0 / 0 | 256 / 0 / 0 | 동일 |
| completion tokens / text chunks | 이전 계측 | 34,693 / 34,016 | chunk와 native token을 구분 |
| 완료 window | 16.527 s | 40.041 s | 출력 길이가 달라 raw wall만 비교하지 않음 |
| completion tokens/s | 이전 계측 | **866.43** | 새 의미론의 기준선 |
| 3090 first compute/token | 0.742 ms | 0.535 ms | microbatch 확대 효과 유지 |
| 4080 terminal compute/token | 2.895 ms | 1.572 ms | microbatch 확대 효과 유지 |
| first downstream wait/token | 3.055 ms | 1.612 ms | 통신 fallback 없이 감소 |
| native observed maximum batch | 64 | 128 | 목표대로 확대 |
| TCP batch fallback | 0 | 0 | 통신 경로 유지 |

이는 128 physical microbatch가 kernel/dispatch 효율을 실제로 높였다는 직접
근거다. 아직 두 context가 같은 GPU를 공유하고 4080 terminal이 상대적으로 더
무겁다. native `n_seq_max`의 실제 상한은 256이므로 다음 단계에서 단일 256
context를 검증했다.

### 1 × 256 physical microbatch: 용량은 통과, 처리량은 하락

`backend-sampler-reverse-14-256-1x256-v2`는 reverse 14:14,
`parallel=concurrent=batch=ubatch=256`으로 실행했다. adapter 수정 후의 strict
trace 검증은 submitted/accepted/done 모두 256, 오류 0, 무결성 위반 0이다.
235개는 `stop`, 21개는 `length`로 끝났으며 native completion token 최댓값은 정확히
500이다.

| 비교: 정확한 P4 completion-token 계측 | 2 × 128 (v4) | 1 × 256 (v2) | 판정 |
| --- | ---: | ---: | --- |
| 완료 / 오류 / 무결성 위반 | 256 / 0 / 0 | 256 / 0 / 0 | 둘 다 유효 |
| completion tokens / text chunks | 34,693 / 34,016 | 56,700 / 55,654 | chunk 수는 token 수와 같을 필요 없음 |
| 최대 completion token | 500 | 500 | 요청 상한 준수 |
| 완료 window | 40.041 s | 107.261 s | 자연 종료 분포가 다름 |
| completion tokens/s | **866.43** | 528.62 | 2 × 128이 63.9% 높음 |
| 3090 first compute/token | **0.535 ms** | 0.614 ms | 2 × 128 우세 |
| 4080 terminal compute/token | 1.572 ms | **1.161 ms** | 단일 큰 batch의 terminal kernel은 효율적 |
| 3090 first downstream wait/token | 1.612 ms | 1.164 ms | terminal wait 자체는 감소 |
| observed maximum batch | 128 | 256 | 256 native graph/sequence 용량 증명 |
| TCP batch fallback | 0 | 0 | transport는 두 경우 모두 비지배적 |

1 × 256에서 두 GPU의 250ms GPU-util 평균은 3090 26.53%, 4080 26.99%였다
(p95 74%, 55%). 이는 "256을 못 묶었다"는 뜻이 아니다. native stage aggregate는
각 70,728 batched tokens와 256 완료 세션을 기록한다. 원인은 235개 요청이 EOG로
종료된 뒤 같은 단일 lane 안에서 active batch 폭이 계속 줄어든 것이다. 종료 길이가
고정되지 않은 production 의미론에서 평균 GPU-util과 전체 처리량을 곧바로
tensor-core 한계로 읽으면 안 된다.

### P4 완료 토큰 의미론 정정

`TOKEN` frame은 UTF-8 text filter가 내보낸 **text chunk**다. 하나의 native token이
여러 chunk가 되거나, 여러 native token이 하나의 chunk로 합쳐질 수 있으므로
`TOKEN` frame 수는 생성 token 수의 권위값이 아니다. 이제 Rust P4 adapter는
마지막 SSE usage의 `completion_tokens`를 `DONE.generated_tokens`로 보낸다. usage가
없는 backend에는 안전하게 chunk 수를 쓴다. 검증 불변식은 `DONE.generated_tokens <=
INGRESS_SUBMIT.max_tokens`이고, chunk 수와의 동일성은 요구하지 않는다.

## 고정 길이 500-token benchmark: 자연 종료 변수를 제거한 재측정

production 요청의 EOG 종료를 바꾸지 않으면서 비교 가능한 물리 batch를 만들기 위해
benchmark에서만 첫 stage가 `--benchmark-ignore-eog`를 받게 했다. terminal이 sampled
EOG를 보더라도 first stage는 EOG가 아니라 `max_tokens`에서만 session을 끝낸다. 이
옵션은 `benchmark=true` 없이는 P4에서 거절하며, 정상 mode의 64-token regression은
EOG에 의해 37 tokens에서 `stop`으로 완료되어 기존 의미론이 보존됨을 확인했다.

처음의 고정 길이 256 run은 supervisor bundle이 domain build보다 오래되어 native flag가
빠진 것을 launch log로 발견했다. 따라서 결과를 폐기하고 `npm run build:server --workspace
llama` 후 bundle에 flag가 들어간 것을 확인한 뒤 재실행했다. 이는 성능 비교의 일부가
아닌 artifact-identity 검증 실패 사례다.

| 구성: reverse 14:14, 요청당 500 native completion tokens | 완료/오류 | 총 tokens | 가장 느린 요청 window | 처리량 | 3090 compute/token | 4080 compute/token | 3090 downstream wait/token | TCP fallback |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 × 256, batch=ubatch=256 | 256 / 0 | 128,000 | 170.650 s | 750.07 tok/s | 0.583 ms | 0.661 ms | 0.662 ms | 0 |
| 2 × 128, batch=ubatch=128 | 256 / 0 | 128,000 | 78.005 s | 1,640.92 tok/s | 0.420 ms | 0.711 ms | 0.714 ms | 0 |
| **4 × 64, batch=ubatch=64** | **256 / 0** | **128,000** | **37.985 s** | **3,369.71 tok/s** | **0.256 ms** | **0.806 ms** | **0.834 ms** | **0** |

1 × 256의 길이 고정 run은 natural drain 때문이 아니라 한 context 안의 stage 실행과
hand-off가 직렬적으로 길어짐을 보였다. `ubatch=128`으로 하나의 256 context를 두
window로 만들려는 별도 시도는 실행 전에 upstream GGML assertion으로 중단됐다.
`n_tokens=128`, `n_seqs=256`에서 runtime이 256으로 올림한 뒤 view 범위를 벗어났다.
이것은 P4 transport 결과가 아니며, upstream/compat 경계를 즉시 바꾸지 않고 지원하지
않는 batch shape로 기록한다.

### 15:13 재균형의 음성 결과

14:14의 4 × 64 결과에서 terminal 4080의 GPU 활성률이 높고 first 3090에는 여유가
있어, wire·sampler·batch·prompt·max token을 그대로 둔 채 first stage에 한 레이어를
더 주는 15:13만 비교했다. 첫 시도는 load profile
`0.8/0.95/40/123`과 request profile `0.2/0.9/20/7`의 불일치로 native가 방어적으로
종료했으므로 성능 결과에서 제외했다. 동일 profile로 재실행한 `v3`만 유효하다.

| 구성: 4 × 64, 256세션, 요청당 500 | 14:14 | 15:13 | 변화 |
| --- | ---: | ---: | ---: |
| 완료 / 오류 / token-cap 위반 | 256 / 0 / 0 | 256 / 0 / 0 | 동일 |
| 총 native completion tokens | 128,000 | 128,000 | 동일 |
| 가장 느린 요청 window | 37.985 s | 37.827 s | -0.4% |
| 처리량 | 3,369.71 tok/s | 3,383.82 tok/s | +0.4% |
| 3090 compute/token | 0.256 ms | 0.277 ms | 악화 |
| 4080 compute/token | 0.806 ms | 0.779 ms | 소폭 개선 |
| 3090 downstream wait/token | 0.834 ms | 0.809 ms | 소폭 개선 |
| 3090 / 4080 GPU 평균 활성률 | 약 42% / 82% | 43.882% / 79.747% | 불균형은 유지 |
| shared-memory frames (lane별) | 560/560 | 559/559 | 정상 |
| TCP batch fallback | 0 | 0 | 정상 |

레이어 한 장을 4080에서 3090으로 옮겨도 최종 stage가 여전히 더 오래 걸리고 전체
처리량 변화는 반복 오차 수준이다. 다음 변경 대상은 layer cut이 아니라 **동일 GPU에서
독립 context들을 어떻게 동시에 CUDA stream에 제출하는지**와 terminal decode의
physical scheduling이다.

## Nsight Systems: 256세션 CUDA 실행·대기열 판정

다음 capture는 위 기준선과 같은 reverse 14:14, 4 × 64, 요청당 정확히 500 native
completion tokens를 사용했다. native stage의 binary stdout은 P4 control stream이므로
개별 `linker-node`를 profiler로 감싸지 않았다. 대신 부모 Node supervisor를 Nsight
Systems의 child-process trace 아래에서 기동해 P4 pipe 의미론을 보존했다. capture
중에도 네 lane은 모두 `P4_SHARED_MEMORY_PASS`(각 559/559 frame),
`P4_PIPELINE_E2E_CLIENT_PASS`, 256/256 완료, 오류 0, TCP fallback 0을 기록했다.

| 항목 | Nsight capture 결과 | 기준선과의 관계 |
| --- | ---: | --- |
| 유효 완료 / 오류 / 총 completion tokens | 256 / 0 / 128,000 | 고정 길이 불변식 유지 |
| wall time / 처리량 | 39.606 s / 3,231.87 tok/s | profiler 부하로 기준선 3,369.71 tok/s보다 4.1% 낮음; 성능 비교값으로 사용하지 않음 |
| 3090 stage 0 compute / downstream wait 합 | 35.334 s / 111.930 s | downstream wait가 stage 0 compute의 3.17배 |
| 4080 terminal stage 1 compute 합 | 109.399 s | 네 terminal context의 지배적 작업 |
| CUDA native process | 8개 (3090 4, 4080 4) | 네 lane의 양 stage가 모두 capture됨 |

`nvidia-smi` 250 ms sample은 3090/4080 평균 41.93%/79.94%였다. 보다 세밀한
Nsight CUDA graph timeline에서는 동일 GPU에 속한 네 context의 실행 구간을 union으로
합쳐야 물리 GPU의 공백을 볼 수 있다. 일반 kernel event만 합치면 CUDA graph 내부
kernel을 별도의 짧은 이벤트로 세어 실제 graph 실행시간을 과소평가하므로, 아래 판정은
`CUPTI_ACTIVITY_KIND_GRAPH_TRACE`의 union을 사용한다.

| 물리 GPU / P4 역할 | graph 실행 union / 관측 span | 점유율 | graph gap p50 / p95 / p99 / 최대 | 해석 |
| --- | ---: | ---: | ---: | --- |
| RTX 3090, first stage | 11.582 / 37.813 s | 30.63% | 12.218 / 50.499 / 91.947 / 143.792 ms | downstream/terminal 대기가 큰 여유로 나타남 |
| RTX 4080, terminal stage | 26.716 / 37.867 s | 70.55% | 1.175 / 17.127 / 36.341 / 500.699 ms | 지배 stage지만 약 29.45%의 graph-level 공백이 남음 |

CUDA runtime call도 terminal에 집중된다. 4080 terminal은 `cudaStreamSynchronize`
449,804회/51.719 s, `cudaMemcpyAsync` 705,319회/15.524 s,
`cudaLaunchKernel` 1,505,700회/15.398 s, `cudaGraphLaunch` 6,027회/9.978 s를
기록했다. 3090 first의 같은 값은 38,008회/11.941 s, 20,956회/0.796 s,
647,472회/5.055 s, 1,660회/0.772 s다. execution summary의 launch queue 평균도
3090 2.470 ms 대비 4080 **12.962 ms**다. 즉 4080은 이미 다중 context의 제출 backlog를
받고 있지만 graph 공백이 없지는 않다.

| GPU / 역할 | CUDA launch 수 | queue가 있는 launch | 평균 queue time | kernel 시간 합 | kernel 시간 상위 2개 |
| --- | ---: | ---: | ---: | ---: | --- |
| RTX 3090, first | 328,328 | 326,364 | 2.470 ms | 4.676 s | `mul_mat_q` 1.897 s, `flash_attn_ext_f16` 0.995 s |
| RTX 4080, terminal | 794,490 | 793,956 | **12.962 ms** | 5.067 s | `mul_mat_q` 2.216 s, `flash_attn_ext_f16` 0.765 s |

이것으로 확정할 수 있는 사실은 다음과 같다.

1. P4 transport는 256세션에서 손실/대체 경로 없이 동작하므로 다음 최적화 대상이 아니다.
2. terminal 4080은 layer cut을 한 장 바꾸어도 해소되지 않은 CUDA graph scheduling과
   synchronization 부담을 갖고 있다. 3090의 낮은 사용률은 terminal 완료를 기다리는
   결과와 일치한다.
3. 다만 70.55% graph 점유율이나 12.962 ms queue는 SM occupancy, tensor-core active
   cycle, memory bandwidth의 측정값이 아니다. 따라서 이것을 tensor-core 포화로
   과장할 수 없으며, hardware counter 없이 커널 자체를 바꾸는 것도 근거가 부족하다.

## 보존 artifact

모든 원시 결과는 `apps/p4/target/pipeline-e2e/`에 남긴다.

- `trace-backend-sampler-14-256-lane{1..4}.jsonl` — 256개 전체 요청·응답 쌍
- `trace-...md`, `report-...md` — 사람이 읽는 요청·최종문장·세션 보고서
- `summary-...json` — stage/wire/latency 집계
- `summary-...-gpu.jsonl` — 250 ms 원시 GPU samples
- `plan-...json`, `client-...log`, `p4-agent-...log`, `p4-pipeline-...log` — 계획과 실행 로그
- `trace-backend-sampler-reverse-14-256-2x128-v4-lane{1,2}.jsonl` — 새 completion-token
  의미론으로 재측정한 256개 요청·응답 쌍
- `summary-backend-sampler-reverse-14-256-2x128-v4-lane{1,2}.json` — 같은 run의
  native stage aggregate, shared-memory 전송과 sampler 통계
- `trace-backend-sampler-reverse-14-256-1x256-v2.jsonl` 및
  `summary-backend-sampler-reverse-14-256-1x256-v2.json` — 단일 native context의
  256 sequence, batch=256 용량과 strict token-cap 검증
- `trace-fixed-length-reverse-14-256-4x64-v1-lane{1..4}.jsonl` 및
  `summary-fixed-length-reverse-14-256-4x64-v1-lane{1..4}.json` — EOG 억제,
  정확히 128,000 native completion tokens의 현 benchmark 기준선
- `trace-fixed-length-reverse-15-256-4x64-v3-lane{1..4}.jsonl` 및
  `summary-fixed-length-reverse-15-256-4x64-v3-lane{1..4}.json` — 동일 workload의
  15:13 재균형 음성 대조; prompt, DONE, stage/wire/GPU 원시 계측 포함
- `summary-fixed-length-*-gpu.jsonl` — 250ms 간격 원시 GPU 사용률·VRAM·전력 샘플.
  모든 valid fixed-length run은 `P4_SHARED_MEMORY_PASS`와 `P4_PIPELINE_E2E_CLIENT_PASS`를
  client log에 함께 보존한다.
- `.cache/p4-nsys-256/p4-fixed-reverse-14-256-4x64-v1.nsys-rep` — 부모 supervisor와
  그 아래 8개 native CUDA process의 Nsight Systems 원본(153.6 MB)
- `.cache/p4-nsys-256/p4-fixed-reverse-14-256-4x64-v1.sqlite` 및
  `analysis_cuda_{kern_exec,gpu_kern,api}_sum.csv` — 이 문서의 graph union, runtime
  API, launch queue 계산에 사용한 재현 가능한 추출본
- `trace-nsys-fixed-reverse-14-256-4x64-v1-lane{1..4}.jsonl` 및
  `summary-nsys-fixed-reverse-14-256-4x64-v1-lane{1..4}.json` — profiler capture 중의
  256개 요청·응답 쌍과 P4 stage/wire 집계

## 2026-08-10: 선택한 P4 개선

256-session trace와 Nsight 결과는 shared-memory P4 transport가 요청 손실 없이
동작하고 TCP fallback이 0임을 보였다. 반면 terminal CUDA 실행에는 남은 유휴 구간이
있지만, CUDA backend만을 전제로 한 llama.cpp compatibility 변경은 아직 동일 fixture의
성능 이득으로 검증되지 않았다. 따라서 이번 단계에서 채택한 유일한 구조 변경은
[`AgentProcessor`](../../p4/runtime/src/agent.rs)의 **ingress execution-credit**이다.

Agent는 이제 ready binding과 NodeSlot permit을 먼저 확보한 뒤에만
`INGRESS_ACCEPTED`를 보낸다. 포화된 요청은 accept 없이 `ERROR`로 끝나므로 external
controller는 수락을 실제 실행 슬롯의 확보로 해석할 수 있다. 이 정책은 opaque
`p4_max_inflight`만 사용하며 CUDA, llama.cpp private ABI, Pipeline hidden-state, sampling
option을 해석하지 않는다. mock adapter TCP test는 ready credit에서
`INGRESS_ACCEPTED → DONE`, saturated credit에서 `ERROR`만 발생함을 검증한다.

기존 CUDA/llama.cpp boundary-copy 변경 후보는 adapter-private 실험으로 남기며, 이
P4 개선의 수용 근거나 protocol requirement가 아니다.

## 다음 스테이지 제안

1. **4 × 64를 고정 benchmark 기준으로 보존한다.** 이것이 현재 유일하게 256개
   요청 모두를 정확히 500 native tokens로 끝내고 3.37k tok/s를 보인 구성이다.
   `benchmark_ignore_eog`는 production 기본값이 아니며, 자연 EOG 허용 run은 별도
   품질/실사용 지표로 유지한다.
2. **다음 최소 측정은 terminal RTX 4080의 Nsight Compute다.** `mul_mat_q`와
   `flash_attn_ext_f16`에 한정하여 SM active, tensor-pipe active, achieved occupancy,
   DRAM throughput을 수집한다. capture는 다시 256/256·128,000 tokens·shared-memory
   pass·TCP fallback 0을 충족해야 하며, profiler 처리량은 기준선과 섞지 않는다.
3. **counter 판정에 따라 변경 대상을 한 곳으로만 좁힌다.** tensor/SM이 높고 DRAM도
   포화면 P4·scheduler를 더 만지지 않고 모델 quantization/placement 또는 더 큰 GPU가
   다음 선택지다. tensor/SM이 낮거나 graph gap과 launch stall이 유지되면 native
   scheduler의 terminal return wakeup, cross-context CUDA graph submit, 동기화 빈도를
   한 변경씩 줄인다. 이 경우에도 P4 wire/공유 메모리 ABI는 바꾸지 않는다.
4. **scheduler 개선은 기준선 전후 1회씩만 재측정한다.** 수용 기준은 256/256 정확
   500-token 완료, shared-memory 순서 보존, TCP fallback 0, terminal graph union 증가,
   stage 0 downstream wait 감소, 그리고 baseline 3,369.71 tok/s보다 재현 가능한
   처리량 증가다. counter와 timeline이 개선되지 않으면 변경을 되돌리고 다른 가설을
   세운다.
5. **그 뒤에만 microbatch/placement를 재개한다.** 한 context에서 `256×128`은 GGML
   assertion으로 현재 지원 불가이고 15:13은 +0.4%라서, 두 실험을 반복하지 않는다.
   scheduler가 고쳐진 뒤에만 4×64 대비 2×128 및 stage cut을 재측정한다.

이 순서는 이미 검증된 P4 중계 경로를 보존하면서, 다음 변경을 CUDA 제출/실행 계층으로
한정한다. 즉 통신 최적화가 끝났다는 선언이 아니라, 통신 가설을 충분한 실측으로
배제하고 GPU scheduler 가설을 다음 대상으로 좁힌 것이다.
