# 2026-09-07 — HEAD `2ed9b71d4` 검증과 3090×2 실기 사다리

종류: HEAD 컴파일·시험·변이 판정과 개발 하네스 실측. 최종 다중 컴퓨터 증명이 아니다.
대상 소스: `2ed9b71d42e5af55a08a75fb2d7f685e038c0874`(작업 트리 clean에서 시작).
현재 작업 순서는 [로드맵](../../../../../../../docs/distributed-batching-roadmap.md),
판정 기준은 [검증 규약](../../../../../../../docs/distributed-batching-verification.md) 소유다.
아래 숫자는 모두 `target/p4-4node/runs/<run>/report.json`·`gpu.csv`·`evidence.json`에서
스크립트로 옮겼다(`runtable.mjs`, 이 세션의 scratchpad). `target/` 원자료는 이 머신의 보존물이다.

## 1. HEAD 컴파일·시험 상태

| 항목 | 결과 |
| --- | --- |
| `cargo build --workspace --release --locked` | 성공(경고만) |
| `cargo check --workspace --all-targets --locked` | **실패** — `p4-llamacpp-staged-adapter` lib test, `v2/node/issue_witness_tests.rs:409` `E0594`: `RequestState`가 `Deref`만 구현(WIP `6fe10eb10`의 불변 입력 공유)인데 시험이 `.template.envelope.return_route = None`으로 직접 대입 |
| `cargo test --workspace --no-fail-fast --locked` | 위 컴파일 오류로 **시험 0개 실행, exit 101** |
| `cargo test --workspace --exclude p4-llamacpp-staged-adapter --no-fail-fast --locked` | 849 passed / 0 failed / 0 ignored, 50 summary, exit 0 |
| 워크트리(별도 checkout, HEAD + 시험 1줄 수정 `.input_mut_for_test()`) `cargo test -p p4-llamacpp-staged-adapter --locked` | 512 passed / 0 failed / 7 ignored, exit 0. `event_actor_ring_saturated_normal_ingress_must_progress_without_external_dequeue` **ok**, cap8 대조 ok |
| 하네스 `node --test test/benchmarks/p4-4node/*.test.mjs` | 72 passed / 0 failed |
| `node tools/scripts/docs-lint.mjs --all` | 79 files clean |
| CUDA staged server CTest(HEAD 소스, Release) | 15/15 passed |

시험 수정은 저장소 작업 트리가 아니라 독립 워크트리에서만 했다. HEAD 자체는 여전히 lib test가
컴파일되지 않는다. 합계 849+512=1361 통과는 두 실행의 합이며 한 번의 `--workspace` 집계가 아니다.

### 1.1 cap1 교착 후보의 변이 판정

`2ed9b71d4`의 `EventNode::forward_independent_front`를 즉시 `Ok(())` 반환으로 무력화한 워크트리에서
같은 두 시험을 실행했다.

- cap1 `..._must_progress_without_external_dequeue`: **FAILED** — `actor_ring.rs:288` "bounded actor-ring state
  observation"(관측 512회 초과) 뒤 `:377` "actual EventNode stopped". 최종 `normal_progress` 단언까지
  가기 전에 상한에서 끊긴 형태다.
- cap8 `..._completes_with_capacity_eight`: ok.

후보 런타임 파일 6개를 통째로 `2ed9b71d4~1`로 되돌린 변이는 시험 래퍼가 새 trait 메서드
(`peek_completion`, `try_take_completion_matching`)를 구현하므로 컴파일 오류로 끝났고 검출로 세지 않는다.
따라서 "후보를 제거하면 cap1이 실패한다"는 의미 변이 1종으로만 확인했다. 시험 파일은 RED 봉인
`393a6c23e` 이후 +118/-49로 바뀌었고 최종 `assert!(normal_progress, ...)`는 양쪽에 그대로 있다.

## 2. 실기 바이너리 신원

| 항목 | 값 |
| --- | --- |
| `p4_staged_server.exe` (HEAD 소스, Ninja, `CMAKE_BUILD_TYPE=Release`, sm 86;89) | `337b09abe257762b053e6d6e70517e2678704a819830437e7ebcfc9d151b4c34` |
| `ggml-cuda.dll` | `ee8de6e0ee017c433f564df246743cf5650808980cf767c14feff81f2eb56e15` |
| `p4-agent.exe` (release) | `bca4da35c9affdbe1330f79414b078e5cc8f7271657f1bb7a2114ecbc7159b29` |
| 이전 배치 exe(09-04, 로컬·원격 동일) | `821694ef4d13d0dff409d825303b91d6337cbcb69e935a52de8eb7206e1a84c9` — `2e9451a5c`의 server C++ 변경 이전 빌드 |
| 원격 launcher `run-agent-42003.cmd` | `675ef882aae5dc609cab2f6da2f268187c01a6c291a82f2da496529f300f7329` (09-04 기준선과 동일: `PREFILL_FRAGMENTS=4`, min/max 0) |

빌드 스크립트를 Ninja generator로 쓰면 `--config Release`만 전달돼 **Debug**가 만들어진다.
Debug smoke는 4.37 gen TPS(`20260907T084050Z-90e71e65`)로 성능 증거에서 제외했다.
원격 배치는 scp 뒤 `Get-FileHash`로 위 해시와 일치함을 확인했고, 각 run의 `evidence.json`에도 같은 값이 있다.

## 3. 실측 결과

로컬 `hikaTR`은 RTX 3090 + RTX 4080(비대칭, 로컬 agent 기본 knob), 원격 `M42-SERVER2`는
RTX 3090×2(한 물리 호스트, launcher knob 위와 같음). 두 대상의 TPS를 서로 비교하지 않는다.
원격 사용률의 `peak`는 sampler가 두 GPU에 같은 값을 보고했다.

표의 `gen TPS`는 `run.mjs::metrics`의 `generation_tps`이며 **품질 판정 전** 생성량을
drive의 해제 완료까지 시간으로 나눈 값이다. H4의 `useful_generation_tps`가 아니다.
`ubatch fill%`는 prefill·decode를 합친 phase 혼합 평균이다.

`NOT_ACCEPTED(agent_stopped)`는 agent 종료 실패의 증거가 아니다. `run.mjs::stopChild`가
`exited && child.exitCode !== null`을 요구하는데, 신호로 죽은 자식은 `exitCode=null`,
`signalCode='SIGTERM'`이다. 이 함수 형태를 독립 자식에 적용하니 13 ms 만에 exit 이벤트를
받고도 false였다. 09-03 로컬 run의 같은 값도 이 오판일 수 있다. 원격 arm이 true인 것은
원격 정지가 SSH 스크립트로 이뤄져 이 경로를 타지 않기 때문이다.

| run | scenario | target | 구조 | 의미 | 수락 | gen TPS | rows/s(legacy total) | 물리 batch | rows/batch | ms/batch | mixed | ubatch fill% | GPU util mean/p50/p90/zero/peak |
| --- | --- | --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| `20260907T085616Z-9fea9f43` | smoke | local 3090+4080 | 1/1 | 1/1 | NOT_ACCEPTED(agent_stopped) | 18.61 | 19.45 | 400 | 1.04 | 53.7 | 0 | 0.2 | GPU0 7.9%/p50 0/p90 34/zero 73%/peak 1133 MiB; GPU1 10.3%/p50 4/p90 33/zero 5%/peak 5433 MiB |
| `20260907T085749Z-5d89fa09` | prefill_mix_2stage | local 3090+4080 | 192/192 | 192/192 | NOT_ACCEPTED(agent_stopped) | 328.85 | 813.08 | 915 | 103.76 | 127.6 | 113 | 20.27 | GPU0 15.6%/p50 0/p90 53/zero 55%/peak 7584 MiB; GPU1 19.3%/p50 5/p90 50/zero 3%/peak 10100 MiB |
| `20260907T090201Z-e02437f1` | pressure | local 3090+4080 | FAILED | - | drive exited 1 — unload is busy | - | - | - | - | - | - | - | GPU0 15.2%/p50 9/p90 40/zero 46%/peak 8131 MiB; GPU1 19.1%/p50 9/p90 46/zero 6%/peak 10755 MiB |
| `20260907T090450Z-50ce3487` | prefill_mix_35b_2stage | local 3090+4080 | 64/64 | 60/64 | NOT_ACCEPTED(meaning,agent_stopped) | 122.43 | 182.21 | 2467 | 22.58 | 123.9 | 0 | 4.41 | GPU0 15.0%/p50 2/p90 63/zero 45%/peak 12722 MiB; GPU1 16.5%/p50 6/p90 53/zero 1%/peak 15517 MiB |
| `20260907T092446Z-5f9b5e0c` | smoke | remote 3090x2 | 1/1 | 1/1 | 수락 | 17.26 | 18.04 | 400 | 1.04 | 57.9 | 0 | 0.2 | GPU0 7.3%/p50 0/p90 32/zero 62%/peak 3655 MiB; GPU1 8.0%/p50 1/p90 33/zero 46%/peak 3655 MiB |
| `20260907T092648Z-0b5e6989` | prefill_mix_2stage | remote 3090x2 | 192/192 | 192/192 | 수락 | 189.75 | 469.15 | 3020 | 31.44 | 67 | 71 | 6.14 | GPU0 24.9%/p50 28/p90 44/zero 22%/peak 8324 MiB; GPU1 28.9%/p50 31/p90 46/zero 18%/peak 8324 MiB |
| `20260907T093145Z-3bf475d1` | pressure | remote 3090x2 | FAILED | - | drive exited 1 — unload is busy | - | - | - | - | - | - | - | GPU0 14.7%/p50 4/p90 42/zero 44%/peak 9107 MiB; GPU1 21.1%/p50 13/p90 55/zero 28%/peak 9107 MiB |
| `20260907T093529Z-8d1cb2a8` | prefill_mix_35b_2stage | remote 3090x2 | FAILED | - | drive exited 1 — node already exists | - | - | - | - | - | - | - |  |
| `20260907T093641Z-5eeea50c` | prefill_mix_35b_2stage | remote 3090x2 | 64/64 | 60/64 | NOT_ACCEPTED(meaning) | 117.07 | 173.16 | 2459 | 22.95 | 132.5 | 0 | 4.48 | GPU0 16.1%/p50 3/p90 63/zero 35%/peak 13936 MiB; GPU1 16.7%/p50 3/p90 57/zero 29%/peak 13936 MiB |
| `20260907T094744Z-91d0cd3d` | vram_31b_2stage | remote 3090x2 | 32/32 | 32/32 | 수락 | 71.59 | 124.3 | 1630 | 13.63 | 109.7 | 22 | 2.66 | GPU0 18.7%/p50 3/p90 48/zero 23%/peak 12858 MiB; GPU1 15.0%/p50 3/p90 42/zero 33%/peak 12858 MiB |
| `20260907T095536Z-fe64b567` | offload_35b_moe_2stage | remote 3090x2 | FAILED | - | drive exited 1 — deadline expired with observation evidence Missing { requests: 32, stage_executi | - | - | - | - | - | - | - | GPU0 11.9%/p50 2/p90 45/zero 30%/peak 3798 MiB; GPU1 11.7%/p50 2/p90 45/zero 37%/peak 3798 MiB |

### 3.1 관찰

- 원격 35B VRAM-only의 품질 판정 전 117.07 gen TPS는 09-04 기준선(116.9~118.5)과 같다.
  이번 HEAD 변경으로 처리량이 오르거나 내린 증거는 없다. 다만 이 값은 비교용 비회귀 관측이지
  paired A/B가 아니며, H5의 승격 판정이 아니다. 09-04의 옛 계산식(decode rows/s)에 맞춘 같은
  실행의 값은 116.87이다. 거부된 4건을 빼고 다시 계산하면 **109.70**이고, 이것도 H1/H4의
  승인 지표가 아니다. 의미 60/64의 거부 4건은 반복 퇴화 1건(req-015, repeat 0.96)과
  한글 비율 0.04~0.11 3건이며, 이 arm은 ChatML 모델(GGUF `tokenizer.chat_template`가 ChatML,
  EOS `<|im_end|>`)에 gemma-4 turn을 보낸다.
- **GPU 사용률은 분석창을 밝혀야 읽을 수 있다.** `gpu.csv` 전체는 모델 적재와 정리 구간을
  포함한다. 같은 CSV를 각 실행의 stage span 최소 `ingress_unix_ms`부터 최대 `forward_unix_ms`까지
  자른 값을 함께 적는다(표본은 KST로 해석한 sampler timestamp 기준).

  | 원격 arm | 전체 캡처 GPU0/1 | 실행창 GPU0/1 | 실행창 0% 표본 | 실행창 표본 수 |
  | --- | ---: | ---: | ---: | ---: |
  | 2B `prefill_mix_2stage` | 24.9 / 28.9% | 32.9 / 38.1% | 4.0 / 3.2% | 775 |
  | 35B VRAM-only | 16.1 / 16.7% | 29.3 / 30.7% | 24.2 / 13.6% | 1,251 |
  | 31B VRAM-only | 18.7 / 15.0% | 42.5 / 34.4% | 1.0 / 2.9% | 688 |
  | 35B expert→CPU | 16.8 / 16.7% | 21.8 / 21.6% | 28.6 / 29.1% | 3,345 |

  이는 분석창 교정이지 성능 변화가 아니다. **준비된 합법적 행이 있는데 장치가 쉰다는 결합
  trace는 없다.** ubatch 채움도 phase 혼합 평균이므로 그대로 유휴 근거가 되지 않는다. 35B
  VRAM-only의 물리 batch를 나누면 decode 평균 15.80행(2,410회), prefill 평균 374.37행(49회)이고,
  resident 32에서는 일반 decode가 모두 준비돼도 512 대비 6.25%다. 유휴의 원인 분해(H5)는 하지 않았다.
- **pressure(256 parallel, 512 요청, 4-stage)**: 09-03에는 5회 통과(398~436 gen TPS)했으나 HEAD는
  두 호스트 모두 UNLOAD가 `unload is busy; active_owners=224(로컬 node-3)/256(원격 node-1)`,
  `requests=0`으로 거부돼 drive가 종료했다. `active_owners`/`active_frontiers` 검사는 `2e9451a5c`가
  `worker/shutdown.rs`에 넣었다. **이 관측으로 "512건 완료 뒤 해제 누수"를 확정할 수 없다.**
  `run/inference.rs`는 ERROR를 받으면 `failure`를 채우고 루프를 벗어나 `Ok(InferenceResult{error})`를
  반환하는데, `run/mod.rs:174`의 호출자는 그 `error`를 보지 않고 UNLOAD를 보내며 그 응답 수신이
  실패하면 `?`로 즉시 종료한다. 그래서 최초 추론 오류와 부분 결과가 사라지고 최종 메시지에는
  UNLOAD busy만 남는다. 추론이 중단된 뒤의 정상적인 busy 거부였을 가능성을 배제할 수 없다.
  RELEASE의 생산자는 drive가 아니라 worker이므로(drive는 `RELEASE_RECEIPT_CONTENT_TYPE` 소비만 한다)
  "drive가 마지막 RELEASE를 늦게 보냈다"는 가설도 성립하지 않는다. 재판정은 최초 오류·부분 결과
  보존을 먼저 고친 뒤에 한다. 원격에서는 실패한 run의 stage 4개가 agent에 남아 다음 run이
  `node already exists`로 즉시 실패했다(`20260907T093529Z`).
- gemma-4-31B(dense, 60층 36/24) VRAM-only 2-stage는 하네스 기준으로 수락됐고 stage log의 가중치·KV가
  모두 `CUDA0`(host 314 MiB)였다. 그러나 **32건 전부**가 응답 앞에 `<|channel>thought\n<channel|>`
  표식을 달고 있고, 32건 전부의 stop이 `length`이며, 3건은 코드 펜스가 홀수로 끝나 잘렸다.
  `judge.mjs`의 통과는 §4의 휴리스틱 통과이지 응답 완결성 승인이 아니다.
- 35B MoE 전문가-CPU 오프로딩을 mmap으로 실행하면 `CPU_Mapped 11.8/11.4 GiB`가 S: 공유에서
  페이지 폴트로 읽히며 30분에 stage 실행 1회로 timeout됐다. `--no-mmap`·60분 timeout으로 재실행한
  결과는 아래 §3.2다.

### 3.2 RAM 오프로딩 arm(`--no-mmap`)

원격 3090×2, HEAD Release 바이너리, launcher 위와 동일. 오프로딩은 `--override-tensor` 패턴
`blk\..*\.ffn_(up|down|gate)_exps.*=CPU`로 소유 층의 routed expert만 host에 두고 계산하며, 라우터·attention·
norm·embedding·KV는 GPU다. 분류는 stage log의 buffer 배치로 확인했다.

| run | scenario | target | 구조 | 의미 | 수락 | gen TPS | rows/s(legacy total) | 물리 batch | rows/batch | ms/batch | mixed | ubatch fill% | GPU util mean/p50/p90/zero/peak |
| --- | --- | --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| `20260907T102817Z-8438348c` | offload_35b_moe_2stage | remote 3090x2 | 64/64 | 64/64 | 수락 | 43.19 | 63.57 | 2462 | 22.52 | 354.3 | 0 | 4.4 | GPU0 16.8%/p50 7/p90 49/zero 41%/peak 3798 MiB; GPU1 16.7%/p50 7/p90 51/zero 40%/peak 3798 MiB |
| `20260907T104752Z-006d6389` | offload_122b_moe_1stage | remote 3090x2 | FAILED | - | drive exited 1 | - | - | - | - | - | - | - |  |
| `20260907T104825Z-53c07be8` | offload_122b_moe_2stage | remote 3090x2 | FAILED | - | drive exited 1 | - | - | - | - | - | - | - | GPU0 0.5%/p50 0/p90 1/zero 77%/peak 1678 MiB; GPU1 0.2%/p50 0/p90 1/zero 84%/peak 1678 MiB |
| `20260907T105822Z-9888cd38` | offload_122b_moe_2stage | remote 3090x2 | FAILED | - | drive exited 1 | - | - | - | - | - | - | - | GPU0 0.3%/p50 0/p90 1/zero 81%/peak 4930 MiB; GPU1 0.3%/p50 0/p90 1/zero 80%/peak 4994 MiB |

- `offload_35b_moe_2stage`(Ornith-1.0-35B, 20/20층, ChatML, 32 seq, 64 요청): 수락, judge 64/64.
  stage log: `CUDA0 model 1237.22/721.90 MiB`, `CUDA_Host model 10701.84/11115.15 MiB`, `CUDA0 KV 425 MiB`×2.
  같은 컷·같은 요청 수의 VRAM-only arm(품질 판정 전 117.07 gen TPS, ms/batch 132.5)보다 43.19,
  ms/batch 354.3으로 2.7배 느리다. judge 64/64는 gemma turn을 보내는 VRAM-only arm의 60/64와
  템플릿이 달라 품질 비교가 아니며, §4의 휴리스틱 통과이지 의미·완결성 승인이 아니다.
  실행창 GPU 사용률은 21.8/21.6%로 이 사다리에서 가장 낮다.
- `offload_122b_moe_1stage`: **실행 불가**. `p4-event-drive`의 `run/config.rs::validate`가 노드 2개 미만을
  거부한다(`20260907T104752Z-006d6389`). 시나리오는 제거했다.
- `offload_122b_moe_2stage` 1차(`20260907T104825Z-53c07be8`, 컷 25/24 over 49): node-0는
  `CUDA0 2417.37 MiB`, `CUDA_Host 40476.50 MiB`로 로드됐으나 node-1이
  `llama-graph.cpp:1529 GGML_ASSERT(0 <= begin && begin < end && end <= n_layer)`로 종료(0xc0000409).
  GGUF `qwen35moe.block_count=49`, `nextn_predict_layers=1`이라 fork의 `n_layer()=48`이다.
  컷을 48층 기준 24/24로 고쳐 재실행했다.
- `offload_122b_moe_2stage` 2차: **BLOCKED**(`20260907T105822Z-9888cd38`, 컷 24/24 over 48). 두 stage 모두 로드됐다 — node-0 `CUDA0 model 2210.30 MiB`, node-1 `CUDA0 model 2983.28 MiB`, `CUDA_Host model 38325.07/38996.04 MiB`, `CUDA0 KV 204 MiB`×2, compute `CUDA0 1122.01/1224.01 MiB`. 그러나 node-1이 `stage_memory_plan.cpp:358` `planned and actual memory differ at entry 1`로 exit 5: host entry의 compute가 계획 107,251,776 B, 실제 142,951,040 B(`sched_reserve: CUDA_Host compute buffer size = 136.33 MiB`)로 달랐고 다른 필드(model 40,186,750,976 B, context 138,936,320 B)는 같았다. node-0는 계획=실제였다. tail stage에서 expert를 host에 두고 계산할 때 host compute buffer 예측이 어긋나는 staged server 문제이며, 이 세션에서 server를 고치지 않았다. 따라서 **122B RAM 오프로딩의 정상 응답·TPS는 미측정**이고, 로드 가능성(host 약 39 GiB/stage, GPU 약 3 GiB/stage)만 확인했다.

### 3.3 실측 판정을 가리는 하네스·drive 결함 (2026-09-08 코드 검토에서 확정)

이 절은 실측 뒤 같은 산출물과 HEAD `0681d1c38` 소스를 다시 대조해 확정한 것이다. 세 결함 모두
**위 표의 실패·수락 표시를 그대로 읽으면 안 되는 이유**이므로 재판정보다 먼저 고친다.

| 결함 | 앵커 | 확인 방법 | 영향 |
| --- | --- | --- | --- |
| 최초 추론 오류·부분 결과가 UNLOAD 실패에 덮인다 | `tools/event-drive/src/run/inference.rs`의 ERROR 분기와 `run/mod.rs:174` 호출자 | 소스 대조. 호출자는 `run.error`를 읽지 않고 UNLOAD를 보내며 그 수신 실패는 `?`로 즉시 종료한다 | `pressure` 두 실행의 실패 원인이 UNLOAD busy로만 남았다 |
| 신호로 죽은 자식을 정지 실패로 판정 | `test/benchmarks/p4-4node/run.mjs::stopChild` | 같은 형태를 독립 자식에 적용해 재현: 13 ms에 exit 이벤트, `exitCode=null`, `signalCode='SIGTERM'` → 반환 false | 로컬 arm의 `NOT_ACCEPTED(agent_stopped)`가 무의미하다 |
| 실패한 run의 stage가 원격 agent에 남는다 | `run.mjs`의 실패 경로에 UNLOAD/정리 없음 | `20260907T093529Z-8d1cb2a8`이 `node already exists`로 즉시 실패 | 이번 사다리는 시나리오마다 agent를 재기동해 우회했다 |

## 4. 이 기록이 증명하지 않는 것

- 다중 물리 컴퓨터 분산(H6), H1의 다양한 corpus·응답 전문 판정, H2의 cold/sustained/recovery 세 모드,
  H5의 paired A/B. 위 표의 arm은 개발 하네스의 단일 실행이다.
- 후보의 일반 교착 자유. cap1 시험 하나와 변이 1종만 확인했다.
- 비교 가능한 성능 개선. 같은 launcher·모델·토폴로지의 09-04 기준선과 같은 값이 나왔을 뿐이며,
  paired A/B가 아니므로 비회귀조차 H5 기준으로 증명한 것이 아니다.
- **응답의 의미와 완결성.** `judge.mjs::DEFAULT_CRITERIA`는 200자·한글 비율 0.3·도메인 용어 4개·
  12자 shingle 반복 0.35·U+FFFD 부재·허용 stop만 본다. 사실관계가 틀린 짧은 설명, 표식이 남은 응답,
  `length`로 잘린 응답이 모두 통과할 수 있다. 이 문서의 `의미 n/m`은 그 휴리스틱 계수다.
- **`pressure`의 원인.** 최초 추론 오류가 UNLOAD 실패에 덮여 사라지므로 현재 산출물로는 판정 불가다.
- **로컬 arm의 agent 정지 여부.** `stopChild`의 신호 종료 오판 때문에 `agent_stopped` 값은 증거가 아니다.
