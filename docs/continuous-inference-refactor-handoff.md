# 연속 분산 인퍼런스 리팩터링 인수인계

2026-08-24 현재의 코드, 생성된 실행 증거, 아직 커밋하지 않은 작업 트리를
함께 설명한다. 이 문서는 성공 사례만 모은 변경 로그가 아니다. 무엇을 왜
바꿨고, 각 증거가 어디까지 증명하며, 다음 작업이 무엇을 통과해야 하는지를
고정하는 인수인계 기준이다.

## 현재 결론

lossless cut-set과 ABI 18 native runtime까지는 구현과 검증이 끝났다. P4는
deployment 제출·취소·결과 전달만 담당하고, llama.cpp adapter/native runtime이
물리 UBATCH, Prefill/Decode 혼합, stage-local state와 샘플링을 소유한다.

Gate A는 통과했다. local 3090 + local 4080의 두 stage가 single-stage baseline과
동일한 UTF-8 응답 `cobalt-314159`를 냈고, native conformance도 F32 cut-set의
descriptor와 payload byte를 그대로 보존했다. 그러나 전체 제품 완료는 아니다.

- canonical 40-request local wave는 40/40 terminal, mixed Prefill/Decode,
  GPU 상한 준수까지 성공했지만 수동 의미 검사는 6/40만 통과했다.
- 따라서 Gate B는 **FAILED**다. 높은 TPS와 40/40 transport는 correctness를
  대체하지 않으며, 규칙에 따라 TUF, 원격 3-stage, placement calibration,
  P4 parity로 진행하지 않았다.
- Qwen3.6-27B Q6_K 2-stage 후보는 요청 제출 전 loaded VRAM이 local 상한을
  넘었다. stock llama.cpp의 model/context/compute breakdown을 native가 직접
  기록하고 stage별 `--vram-limit-mib`를 ready 전에 검사하도록 보강했지만,
  GGUF-only planner가 load 전 exact compute graph 크기를 예측하는 것은 아니다.
- 원격 3090 2장 호스트의 Event 41 전원 실패와 TUF SSH 손상 상태는 그대로다.

현재 상태는 **Gate A까지 정확성이 입증된 구현 중간점**이다. 다음 승인은
40/40 의미 정확성을 만족할 수 있는 local acceptance model/workload와 exact
compute-buffer memory plan을 먼저 결정해야 한다.

## 독립 재감사 기록 — 2026-08-24

이 기록은 위의 이전 실행 수치를 재인용한 것이 아니라, 같은 dirty worktree
(`p4/adapter-boundary`, `ba46d6300ccb1bfead90f4f8f4a66d6a27cc4d07`)에서
새로 만든 산출물과 실행의 결과다.

- compat prepare는 official `3e3a7a416d`와 patch-set
  `817426f81c890002708c50c0a5deb6905a3f4348b92198481984d58bf3e88626`를
  검증했다. upstream은 Linker 소스가 없는 pristine tree여야 한다.
- `packages/llama_domain`을 먼저 build한 뒤 `apps/llama` bundle과 CUDA
  runtime을 다시 만들었다. 최종 ABI 18 `linker-node` SHA-256은
  `a9c34e9f9465a011c902ae9fd1a47efa035df5875d1f5fdc830bc42bbc301a09`이고,
  build ID는
  `linker-pipeline-3e3a7a416d65-3e3a7a416d.817426f81c890002708c50c0a5deb6905a3f4348b92198481984d58bf3e88626-78b7fe9a18aa`다.
  server bundle SHA-256은
  `02a28af41b93b9ff34e5e4c8c8c2ed6cc1d26aef8ee1fbdbf0d4c837da775187`이며,
  `/api/runtime`가 보고한 값과 실제 파일 hash가 일치했다.
- build 중 발견한 [`build-node-runtime.ps1`](../../llama/scripts/build-node-runtime.ps1)
  pack manifest 결함도 수정했다. 기본 상대 build directory에서는 absolute
  artifact path를 잘못 잘라 `files[].path`가 runtime-relative가 아니었다.
  runtime root를 absolute path로 정규화하고 회귀 검사를 추가했으며, 새 pack의
  26개 file path는 모두 runtime-relative다. 이 수정도 아직 dirty worktree에 있다.
- unit 재검증은 direct-pipeline 57/57, `llama_domain` 132/132,
  `apps/llama` server 96/96 + scripts 22/22였다. 이는 GPU 정확성의 증거가 아니다.

### Gate A 단일 요청: lossless ABI 18로 통과

최초 Gate A는 실패했다. byte-level 감사에서 generation cut-set을 stock F32에서
F16으로 내렸다가 다시 F32로 올리는 제품 경로가 확인됐고, single-stage와
two-stage greedy token stream이 달랐다. 이 손실 변환을 제거하고 tensor
descriptor와 payload를 그대로 전달하도록 ABI를 17에서 18로 올렸다.

같은 decode helper를 실제 receive path와 native conformance가 공유하도록 바꿨다.
conformance는 F32 descriptor, element count, payload byte equality를 검사하며
`exact_physical_cutset=true`로 통과했다. 이전 `hidden_f16_*` 관측 key는
`hidden_tensor_*`로 바꿨고 persisted history reader에만 legacy fallback을 남겼다.

고정 semantic oracle prompt를 최신 single-stage와 local two-stage (`[0,18)`,
`[18,28)`)에 각각 한 번 제출했다. 두 실행 모두 9 token, `stop`, UTF-8 bytes
`cobalt-314159`였고 server/native/dirty-diff hash도 같았다. 최신 two-stage
evidence SHA-256은
`215ea62ee806ac9095fb2ca31a104b412bbe8205712f0abd8833e3225741d6cb`다.

감사 중 물리 ceiling 없이 기본 균등 `[0,14)`, `[14,28)` split을 만든 비교는
`RESULT = 314159`를 내 의미 검사를 실패했다. 이 실행도 삭제하지 않았다.
cut-set이 다른 실행은 회귀 비교가 아니며, split 자체가 correctness 입력임을
증명한다. 따라서 local multi-stage 준비는 이제 stage마다 물리 VRAM ceiling을
요구하고, 근거 없는 균등 split은 요청 생성 전에 fail-closed 한다.

- [single-stage baseline report](../../../target/gate-a-lossless-abi18/baseline/report.json)
- [two-stage report](../../../target/gate-a-lossless-abi18/two-stage/report.json)
- [latest single-stage baseline](../../../target/final-gate-a-abi18/baseline/report.json)
- [latest proven 18/10 split](../../../target/final-gate-a-abi18/two-stage-proven-split/report.json)
- [preserved rejected 14/14 split](../../../target/final-gate-a-abi18/two-stage/report.json)

### Gate B local 40-request wave: 구조 통과, 의미 정확성 실패

Qwen3.5-9B Q5_K_S, `parallel=40`, `batch=ubatch=128`, local two-stage
(`[0,22)`, `[22,32)`)로 canonical 20 + 5 + 5 + 5 + 5 wave를 실행했다.
40/40이 `stop`으로 끝났고 40,600 token, aggregate 92.111 token/s였다.
모든 후속 wave가 기존 Decode와 겹쳤고 두 stage 모두 8개의 mixed sample을
기록했다. peak VRAM은 3090 10,485 MiB, 4080 9,370 MiB로 상한 안이었다.

그러나 40개 응답 전문을 모두 읽은 hash-bound review는 6/40만 통과했다.
금지된 임의 수치 생성, offset commit 결과 역전, autovacuum lock 오해,
headroom 중복 계산 같은 의미 오류가 반복됐다. 자동 형식 검사는 통과했지만
수동 검사는 완전하고 evidence hash도 일치한 상태에서 `passed=false`다.

- [Gate B report](../../../target/gate-b-local-abi18/inference/report.json)
- [Gate B complete responses](../../../target/gate-b-local-abi18/inference/evidence.md)
- [Gate B manual review](../../../target/gate-b-local-abi18/inference/manual-semantic-review.json)
- [Gate B semantic verdict](../../../target/gate-b-local-abi18/inference/semantic-review.json)

더 큰 Qwen3.6-27B Q6_K 후보는 `batch=ubatch=768` load 직후 3090
24,253 MiB, 4080 15,511 MiB로 각각 23,552/12,288 MiB 상한을 넘었다.
요청은 한 건도 제출하지 않고 runtime을 삭제했다. stage별 graph compute
buffer가 약 12.9 GiB씩 예약됐지만 current planner는 GGUF/KV persistent memory만
소비한다. 이 결함을 고치기 전에는 같은 모델 폭을 줄여 재시도하지 않는다.

## 자기감사: 초반 확답과 실제 결과의 차이

초반에 이 목표를 구현할 수 있다고 확답한 것은 부정확했다. source에서 가능한
구조를 본 것과, 현재 제품 경로에서 정확성·성능·일반성을 함께 증명한 것을
구분하지 않았다. 새 세션은 이 문서의 결론도 주장으로만 취급하고 아래 항목을
코드와 새 실행으로 다시 판정해야 한다.

| 초반 기대 또는 중간 주장 | 현재 실제 상태 |
| --- | --- |
| P4와 llama adapter를 완전히 분리할 수 있다 | 새 deployment relay와 contract는 생겼지만 legacy Hop/window code가 남아 있고 운영 parity로 제거 가능성을 증명하지 못했다. |
| llama.cpp adapter가 연속 혼합 배치를 효율적으로 수행한다 | canonical 40요청에서 구조·지속 혼합·transport는 통과했지만 의미 정확성은 6/40이라 성능 승인이 막혔다. |
| 여러 GGUF architecture를 일반적으로 지원한다 | model-name 분기를 거부하고 GGUF recurrent metadata를 읽으며 여러 architecture inspect는 통과했다. 모든 architecture의 실제 generation을 증명한 것은 아니다. |
| stage-local KV/recurrent memory가 완성됐다 | 최신 source/dist/native load와 stock breakdown 계측은 통과했다. Qwen2 transformer의 실제 2-stage는 통과했지만 recurrent architecture 실기 generation은 아직 승인되지 않았다. |
| 4-GPU 성공이 현재 구현을 증명한다 | 일부 실행은 stale binary였고, 250 W 실행은 전원 원인 분리를 위해 의도적으로 이전 binary와 고정 plan을 재사용했다. 현재 dirty source의 증거가 아니다. |
| terminal 40/40이면 인퍼런스가 성공했다 | 최신 local Gate B 수동 의미 검사는 6/40이었다. transport 완료와 올바른 추론을 혼동한 실패다. |
| 원격 준비는 단순한 후속 작업이다 | direct preparation은 동적 GPU 수와 2+1 topology를 지원하지만 TUF SSH와 실제 supervisor-owned model load는 아직 막혀 있다. |

반복된 판단 오류도 보존한다.

- counter `0`, process exit, listener, configuration, 파일 존재를 실제 data-path
  실행 증거로 오인했다.
- source를 수정한 뒤 stale binary가 선택되는 runner를 먼저 봉인하지 않아
  무관한 실행을 새 코드의 성공으로 보고했다.
- terminal/non-empty 검사를 응답 의미 정확성으로 확대 해석했다.
- llama.cpp와 기존 native scheduler를 충분히 읽기 전에 P4에서 이미 해결된
  batching을 다시 구현하려 했다.
- VRAM만 보며 hardware power envelope를 별도 안전 조건으로 두지 않았다.
- TUF의 기존 SSH key file을 백업·식별하기 전에 수정했고, 줄바꿈 없는 key
  append와 PowerShell scalar concatenation으로 기존 접속까지 손상시켰다.

따라서 이 작업의 성과를 “초고성능 분산 인퍼런스 구현”이라고 부르면 안 된다.
현재 인정할 수 있는 성과는 boundary/relay/native scheduler/planner/evidence의
부분 구현과 여러 실패 원인의 식별이다. 제품 성공 판정은 완료 정의를 새
산출물로 모두 통과한 뒤에만 가능하다.

## 최종 책임 경계

| 소유자 | 소유하는 것 | 소유하지 않는 것 |
| --- | --- | --- |
| P4 core | deployment 발견과 identity, bounded relay, `Submit`/`Cancel`, `Accepted`/`Rejected`/`Produced`/`Settled` 라우팅 | rank, layer, KV, Prefill, Decode, Hop, Window, UBATCH, 모델별 분기 |
| llama deployment client | 지속 multiplexed 연결, generation fencing, reconnect/replay, cancel 보존, bounded queue/ledger, `Full` deadline | 물리 배치 편성, tensor shape 재해석 |
| `apps/llama` | GGUF inspection, runtime pack과 프로세스 수명주기, placement 후보·검증, 실행 통계 집계 | P4 라우팅 정책, 모델명 기반 실행 분기 |
| native `linker-node` | ready session, 물리 llama.cpp UBATCH, Decode 행 예약 뒤 남는 행의 Prefill water-fill, 단계별 KV/recurrent state, exact cut-set, terminal sampling | P4 admission과 외부 deployment identity |
| stock llama.cpp | 모델 architecture graph, tensor role/shape, 실제 physical ubatch, backend/RPC/GPU 실행 의미 | Linker/P4 전용 프로토콜과 상태 |
| benchmark harness | 동일 workload, binary/source hash, GPU·VRAM·power 표본, 응답 전문과 수동 의미 판정 | 런타임 정책, 결과를 성공으로 해석하는 우회 규칙 |

현재 경계의 대표 구현은
[`agent/relay.rs`](../layers/agent/src/agent/relay.rs),
[`deployment/client.rs`](../layers/adapters/llamacpp/deployment/src/client.rs),
[`entrypoints deployment.rs`](../entrypoints/agent/src/adapters/deployment.rs)에 있다.
P4 relay는 hop을 만드는 경로의 대안이며 phase를 분류하지 않는다. required
deployment 주소가 없으면 기동을 실패시켜 새 경로를 시험하면서 legacy 경로로
조용히 빠지는 것을 막는다.

## 수정한 내용과 이유

### 1. P4의 backend-shaped 경계를 deployment submission으로 축소

초기 P4에는 Prefill/Decode lane, Hop/Window, position·remaining token 같은
llama.cpp 실행 지식이 core까지 올라와 있었다. 이 구조에서는 새 모델과 새
batch 알고리즘을 도입할 때마다 P4 protocol과 scheduler까지 바뀌고, P4가
어댑터보다 적은 정보로 물리 배치를 다시 결정하게 된다.

이를 deployment-scoped `Submit`/`Cancel`과
`Accepted`/`Rejected`/`Produced`/`Settled` 계약으로 바꿨다. P4 payload는
prompt, 최대 출력 길이, 불투명 options를 운반하고 OpenAI/llama 요청 모양은
어댑터에서 만든다. `Full`도 문자열 오류가 아니라 typed refusal로 처리한다.

이 변경의 목적은 P4를 무용하게 만드는 것이 아니다. P4가 반드시 알아야 하는
분산 control-plane 성질만 남기고, GPU 실행 지식은 더 많은 정보를 가진
어댑터로 내리는 것이다.

### 2. relay를 운영 가능한 bounded stream으로 강화

지속 연결과 재접속 경로에는 다음 결함이 있었다.

- socket write가 막히면 submit 호출 자체가 막힘
- reconnect의 snapshot/install 사이에서 submission이 0회 전송될 수 있음
- control과 data가 같은 bounded queue를 써 `Cancel`과 generation advance가
  조용히 유실됨
- 반대로 control 유실을 없애기 위해 채널을 무제한으로 바꾸면 메모리가
  무제한 증가함
- cancel intent가 ledger에 없어서 write 실패 뒤 reconnect 시 유실됨
- stale reader event에 connection epoch가 없어 새 연결의 event처럼 처리됨
- `Full` 재시도가 deadline 없이 영원히 지속될 수 있음
- backend가 발급해야 할 generation을 P4가 기본값 `1`로 발명함

single-owner pump, submission permit, bounded inbound/ledger, cancel intent replay,
connection epoch, backend-owned generation handshake, reconnect prefix 중복 흡수,
deadline이 있는 `Full` 재시도를 도입했다. 특히 reconnect 뒤 이미 받은
`Produced` prefix는 duplicate로 버리고 forward gap만 오류로 처리한다.

이 경로는 unit 초록만으로 봉인하지 않았다. reconnect·cancel·동시 상한에서
해당 가드를 제거하면 실패하는 mutation과 실소켓 테스트로 성질을 확인했다.

### 3. lifecycle/identity 결함을 먼저 고정하고 legacy checkpoint로 남김

EOS slot 누수, position에서 index 재도출, terminal accounting 누락, sequence
재사용 중 늦은 close가 새 세션을 취소하는 문제가 있었다. response event
ordinal, session epoch, terminal accounting, close acknowledgement와 bounded
tombstone을 도입해 당시 경로의 수명주기 결함을 막았다.

이 작업은 배치 구현이 아니라 기존 경로의 capacity/lifecycle 복구였다.
`b0bc5b06f`는 그 사실을 보존한 checkpoint다. 최종 deployment submission
계약에서는 `Settled`가 submission terminal이므로, 이 checkpoint의
`SessionClose/session_epoch` 상당 부분을 최종 소유권으로 오해하면 안 된다.

### 4. 물리 batching을 llama.cpp 어댑터 뒤로 이동

기존 P4 window는 lane을 섞지 않았지만 native path는 이미 한 물리 UBATCH에
Prefill과 Decode를 함께 넣는 구조였다. 배치 최적화에 필요한 ready session,
KV residency, prompt 잔량, llama.cpp가 실제로 쪼갠 microbatch는 P4가 알 수
없다. 따라서 최적화는 native 어댑터가 소유하도록 옮겼다.

현재 [`session.inc`](../../llama/native/linker-node/inference/session.inc)는
한 scheduler window를 한 physical llama.cpp UBATCH로 취급한다. 먼저 active
Decode sequence에 한 행씩 예약하고, 남는 행을 Prefill에 배정한다.
[`prefill-plan.inc`](../../llama/native/linker-node/inference/prefill-plan.inc)는
한 긴 prompt가 전부를 독점하지 않도록 residual 행을 water-fill한다.
[`physical-window.inc`](../../llama/native/linker-node/inference/physical-window.inc)는
llama.cpp가 실제로 만든 physical microbatch와 logical owner를 대응시키며,
[`batch-forward.inc`](../../llama/native/linker-node/pipeline/batch-forward.inc)는
모든 downstream stage가 같은 physical capsule을 실행하게 한다.

그 결과 단일 요청은 배치가 덜 찬 정상 사례로 처리되고, 다중 요청에서는
Decode를 굶기지 않으면서 남는 GPU 폭을 Prefill로 채울 수 있다. 이것은
“Prefill 전용 노드”와 “Decode 전용 노드”를 고정하는 정책이 아니다. 매 window의
실제 ready state에 따라 두 phase를 함께 편성한다.

### 5. stage-local KV와 recurrent memory로 수정

KV cache를 pipeline 전체 크기로 각 노드에 중복 할당하거나, adapter가 모델
이름을 보고 특례를 만드는 것은 모두 잘못이다. 각 stage는 자신이 소유한
layer의 mutable state만 가져야 한다. tensor 역할과 크기의 권위는 GGUF와
stock llama.cpp architecture graph여야 한다.

stage별 contiguous layer range에 맞춰 KV를 할당하고, recurrent 모델은 GGUF의
SSM convolution/state/inner/group metadata에서 정확한 R/S width를 계산하도록
일반화했다. metadata가 불완전하면 모델명 추정으로 통과시키지 않고 fail-closed
한다. 상세 계약은 [llama.cpp stage memory](llamacpp-stage-memory.md)와
[`apps/llama` internals](../../llama/docs/internals.md)에 있다.

현재 source, `packages/llama_domain/dist`, `apps/llama` server bundle과 ABI 18
native runtime은 다시 빌드됐다. Qwen2.5 transformer의 최신 local 2-stage load와
단일 요청은 통과했다. recurrent R/S width는 parser/unit/compat source까지
검증됐지만 recurrent 모델의 최신 stage generation은 Gate B 중단선 뒤이므로
아직 제품 승인 증거가 아니다.

### 6. placement를 VRAM 비율이 아닌 측정 가능한 비용으로 전환

`12:23:23:23` VRAM 비율이나 “4080은 무조건 선두” 같은 고정 규칙을 제거하는
방향으로 바꿨다. 실제 배치는 다음 비용을 함께 봐야 한다.

- GGUF에서 얻은 stage weight와 exact KV/recurrent cache
- 선두 embedding, 말단 norm/LM head 같은 고정비
- boundary activation과 transport cost
- 장치별 실제 free VRAM과 안전 상한
- stage별 `prefill_compute_us`, `decode_compute_us`

planner는 후보를 만들고, ready 뒤 `nvidia-smi`의 실제 증가량으로 상한을 다시
검증해야 한다. 현재 검증 topology는 local 3090 23 GiB, local display-owning
4080 12 GiB, TUF 4070 Laptop은 `min(8,192 MiB, 장치가 보고한 전용 VRAM)`이다.
TUF의 display는 integrated AMD GPU가 담당하므로 4070이 보고하는 전용 VRAM
전체를 inference 상한으로 사용할 수 있다. 단순
용량 비례는 1차 후보일 뿐, 최종 split과 rank 순서는 단계별 compute time,
edge 고정비, remote boundary 전송비를 함께 균형화해 정한다.

TUF의 interactive 사용자 화면에서 `S:\models`가 연결된 것은 확인됐지만 SSH
세션의 drive visibility는 runtime 권위가 아니다. 중앙 PC의 현재 non-interactive
SSH key는 TUF에서 거부됐다. 다음 준비 작업은 접속 통로를 복구한 뒤 SSH로
`S:`를 검사하는 것이 아니라, TUF의 실제 supervisor/native process에 동일한
`S:\models\...` 인자를 전달하고 model-ready가 되는 것을 기록하는 것이다.

### 7. 테스트를 “프로세스가 돌았다”에서 실제 증거로 변경

오래된 고정 경로의 바이너리가 선택되어 새 코드를 전혀 실행하지 않고도
40/40이 보고된 적이 있었다. output file이 없거나 URL만 있는 응답도 빈 본문
검사만으로 통과할 수 있었다. 그래서 harness에 다음을 추가했다.

- source HEAD, dirty path, dirty diff hash, untracked source hash, harness file,
  실제 실행 server bundle과 native hash 및 build id
- 실제 선택한 binary path·mtime·hash
- request마다 다른 긴 prompt, deterministic sampling과 응답 전문
- 세션별 terminal/토큰/본문, wave overlap, phase별 compute time
- GPU별 utilization, VRAM, power 표본
- 사람이 직접 모든 출력 문구를 읽고 판정한 semantic review

정식 목표 workload는
[`direct-pipeline/README.md`](../../../test/benchmarks/direct-pipeline/README.md)의
20개 동시 출발 + 60초마다 5개씩 4회, 총 40요청이다. 입력은 약 480 token,
출력 상한은 2048 token이며 prompt가 충분한 출력을 유도하되 길이를 강제하지
않는다. 이는 기존 Decode가 진행되는 동안 새 Prefill wave가 들어오는 상황을
만들기 위한 계약이다.

## 현재까지의 실행 증거

| 실행 | 결과 | 증명하는 것 | 증명하지 않는 것 |
| --- | --- | --- | --- |
| 5요청 동시, Qwen3.6-27B Q6_K, 4 GPU | 5/5, 7,103 생성 token, 594.777 s, 11.942 aggregate tok/s, first-token 평균 5.369 s | 4-stage 실행, 5-session physical batching, stage 0/1에서 `mixedSamples=1`, 최대 sampled UBATCH 682 | 40요청 wave, 지속 포화, 최신 uncommitted source |
| 10요청 동시, remote 3090 350 W | 10/10 error, 생성 0; 원격 Windows Event 41, bugcheck 0 | host가 부하 중 비정상 전원 상실/리셋됨 | PSU·케이블·GPU 중 정확한 원인 |
| 같은 10요청, remote 3090 250 W | 10/10, 13,228 생성 token, 522.821 s, 25.301 aggregate tok/s | 같은 binary·plan·workload가 낮춘 power limit에서 완주, 4 GPU 모두 peak util 83--100% | 250 W가 영구 해법임, 40요청 wave, mixed phase 지속성, 최신 source |

250 W 실행의 compute 계측은 prompt 3,482 token, Prefill 3,472 token,
`prefill_compute_tps=1758.916`, Decode 13,228 token,
`decode_compute_tps=55.582`였다. GPU peak VRAM은 local 3090 9,701 MiB,
local 4080 12,061 MiB, remote 3090 각각 10,804 MiB였다. 4080은 12 GiB
상한 아래였지만 여유가 약 227 MiB뿐이므로 더 큰 계획은 ready 뒤 실측 없이
허용하면 안 된다.

이 실행은 첫 10요청을 한 번에 보낸 burst라 stage 0/1의 sampled batch가 모두
Prefill이었고 `mixedSamples=0`이었다. 반면 5요청 실행의 `mixedSamples=1`은
혼합 능력의 존재만 증명한다. 둘 다 연속 웨이브에서 혼합률과 GPU 포화가
유지된다는 성능 증거는 아니다.

응답 전송은 10/10 성공했지만 수동 의미 판정은 4/10이었다. 2048 token에서
문장이 잘리거나, prompt가 금지한 임의 수치·시도 횟수를 만들거나, residual
Prefill 규칙과 반대되는 답을 한 사례가 있었다. 따라서 이 실행은 pipeline
transport와 compute의 성공이지 인퍼런스 품질의 성공이 아니다.

로컬 생성 증거:

- [5요청 report](../../../target/direct-pipeline-distributed/qwen36-27b-wave-p5/inference/report.json)
- [10요청 250 W report](../../../target/direct-pipeline-distributed/qwen36-27b-wave-p10-power250/inference/report.json)
- [10요청 응답 전문](../../../target/direct-pipeline-distributed/qwen36-27b-wave-p10-power250/inference/evidence.md)
- [10요청 수동 의미 판정](../../../target/direct-pipeline-distributed/qwen36-27b-wave-p10-power250/inference/manual-semantic-review.json)
- [350 W host failure 진단](../../../target/direct-pipeline-distributed/qwen36-27b-wave-p10/failure-diagnosis.json)

`target/` 증거는 로컬 생성물이며 Git에 보존된 증거가 아니다. 다음 승인 실행은
동일 내용을 repository-owned evidence 위치에 복사하고 hash로 연결해야 한다.

## 변경 이력의 의미

| 범위 | 대표 checkpoint | 의미 |
| --- | --- | --- |
| generic boundary와 event accounting | `b6e16941f`--`b94156408` | backend shape와 silent lap을 P4 contract에서 제거 |
| terminal/capacity 복구 | `096c9c2b7`, `b0bc5b06f` | legacy staged path의 누수·재사용 fence 봉인 |
| deployment relay | `d8f603e7c`--`f663dff7b` | 실제 운영 경로, boundedness, reconnect, generation, deadline |
| 책임 분리 | `848619ea3`, `e29db45d8` | deployment-owned inference와 adapter-owned mixed batching |
| physical UBATCH | `ff870d7ec`, `9172729cb` | residual Prefill water-fill과 continuous staged execution checkpoint |
| model-agnostic memory | `71b6d7307`--`c94acf2e4` | stage-local mutable state, model-name branch 거부 |
| 재현 가능한 evidence | `97eb79f08`--`ba46d6300` | sampling, phase TPS, accelerator, opaque remote model root |

## 재개 체크포인트 — 2026-08-24

새 세션은 이 절부터 시작한다. 기존 dirty worktree는 모두 현재 작업에 속하므로
reset, checkout, broad clean을 하지 않는다.

- branch: `p4/adapter-boundary`
- base HEAD: `ba46d6300ccb1bfead90f4f8f4a66d6a27cc4d07`
- 상태: 문서, source, compat patch, benchmark harness가 모두 uncommitted dirty
  worktree에 있다. 같은 worktree가 아니거나 clean clone이면 이 체크포인트를
  재현할 수 없다.

### 현재 초록

| 검증 | 결과 |
| --- | --- |
| `node --test test/benchmarks/direct-pipeline/*.test.mjs` | 57/57 |
| `npm test --workspace packages/llama_domain` | 132/132 |
| `npm test --workspace apps/llama` | server 96/96 + scripts 22/22 |
| docs graph `--include-apps` | violations 0 |

direct preparation의 request, placement, runtime, `nvidia-smi` parser와 host
bootstrap은 local·remote 각각 한 개 이상의 실제 GPU를 받아 stage 수를 동적으로
만든다. 2 local + 1 remote topology와 host boundary 보존도 테스트로 고정됐다.
VRAM/reserved/compute 배열은 발견한 물리 GPU 수와 정확히 같아야 한다.

[`run-ssh-forwarded-real-four-node.ps1`](../tools/scripts/e2e/run-ssh-forwarded-real-four-node.ps1)는
이름과 verdict가 네 process에 고정된 과거 E2E 기록용 진입점이다. 현재 TUF 2+1
acceptance 경로로 사용하지 않으며, 새 direct preparation을 우회하는 제품 runner가
아니다.

### 현재 중단점

1. Gate B canonical local wave는 구조상 40/40을 완료했지만 hash-bound 수동 의미
   판정이 6/40이라 **FAILED**다. 이 실패가 원격·성능 승인보다 먼저다.
2. 최신 source/dist/server/native provenance와 Gate A는 일치한다. 다만 27B
   후보는 load 후 exact compute graph를 포함한 실제 VRAM이 상한을 넘었고,
   9B 후보는 의미 gate를 통과하지 못했다.
3. 전원 원인 분리용 10요청 실행은 이전의 정확히 고정한 binary/plan을 사용했다.
   현재 dirty source의 product proof가 아니다.
4. TUF `admin@192.168.0.17`은 TCP 22에는 도달하지만 현재 중앙 PC의
   `BatchMode` public-key 인증을 거부한다. TUF의 effective `sshd_config`는
   `C:\ProgramData\ssh\administrators_authorized_keys`를 읽는다. 해당 파일에
   기존 키와 추가 키가 줄바꿈 없이 연결돼 한 키의 comment처럼 해석된 것이
   원인이다. 키 record를 `ssh-ed25519`마다 한 줄로 분리하고 중복 제거한 뒤
   `sshd`를 재시작하면 된다. 이 복구는 로컬 게이트의 선행 조건이 아니다.

### 즉시 재개할 일

원격 일반화나 SSH 복구부터 하지 않는다. Gate A는 끝났고 Gate B correctness가
현재 중단선이다.

1. 6/40 실패 응답과 model/workload 적합성을 먼저 판정한다. transport 성공을
   모델 능력의 성공으로 바꾸는 threshold 완화나 자동 재시도는 하지 않는다.
2. load 전 GGUF persistent plan과 load 후 stock llama.cpp
   model/context/compute breakdown을 구분한다. 각 stage의 실제 breakdown이 물리
   ceiling을 넘으면 ready 전에 실패한다.
3. 40/40 의미 정확성을 만족할 local-fit model/workload를 결정한 뒤 같은 trace를
   한 번 실행한다. 모델명 분기나 임의 split은 추가하지 않는다.
4. local Gate B가 통과한 뒤에만 TUF SSH, supervisor-owned 3-stage model load와
   TCP boundary로 진행한다.

### 새 세션의 첫 감사 순서

1. 이 문서의 수치를 인용하지 말고 `git status`, diff, 현재 binary hash부터
   다시 수집한다.
2. [`manifest.json`](../layers/adapters/llamacpp/staged/compat/3e3a7a416/manifest.json)의
   patch hash와 실제 prepared tree가 일치하는지 확인한다. upstream에는
   Linker 변경이 없어야 한다.
3. 아래 세 테스트를 다시 실행한다. 초록은 unit 성질만 증명하며 GPU 성공으로
   보고하지 않는다.

   ```powershell
   node --test test/benchmarks/direct-pipeline/*.test.mjs
   npm test --workspace packages/llama_domain
   npm test --workspace apps/llama
   ```

4. `npm run build --workspace apps/llama`와 현재 CUDA native build의 실제 출력
   위치·hash·`--runtime-info`를 기록한다. build script가 성공했다는 문구만
   믿지 않는다.
5. latest Gate A의 입력·출력 전문과 source/dist/server/native/diff hash가 모두
   묶였는지 확인한다. Gate B가 실패한 동안 remote/TUF나 calibration을 하지 않는다.

새 세션의 첫 질문은 “어떤 코드를 더 만들까”가 아니라 “현재 dirty source가
실제로 하나의 binary가 되어 올바른 응답 하나를 내는가”여야 한다.

## 다음 작업: 세 개의 통합 게이트

중간 단계가 독립 목표로 증식하지 않도록 다음 세 게이트만 사용한다. 각 게이트는
하나의 실행 가능한 산출물과 명시적인 중단 조건을 가진다.

### 게이트 A — 최신 산출물의 결정론적 일치와 단일 요청 정확성

1. 현재 GGUF/recurrent/planner/source를 먼저 빌드하고, 그 source에서
   TypeScript `dist`, server bundle, native `linker-node`를 순서대로 만든다.
2. report에 source HEAD+dirty diff hash, dist hash, native/server hash와 build id를
   넣고, source보다 오래된 산출물은 실행 전에 실패시킨다.
3. 로컬 `S:\models`의 여러 GGUF architecture를 inspect해 adapter에 모델명
   분기가 없고 필요한
   metadata가 완전하거나 명시적으로 unsupported인지 확인한다.
4. 먼저 local 3090 + local 4080 두 stage에서 planner가 exact stage
   weight/cache를 계산하고 ready 뒤 실제 VRAM 증가가 23/12 GiB 상한 이내인지
   확인한다.
5. parallel 1의 긴 prompt를 실행해 입력, 출력 전문, Prefill/Decode TPS를 사람이
   읽는다. 의미가 틀리거나 문장이 token ceiling에서 잘리면 실패다.

게이트 A가 끝나기 전에는 성능 sweep을 하지 않는다. 잘못된 산출물을 더 큰
부하로 반복하는 것은 증거를 늘리지 않는다.

### 게이트 B — 연속 혼합 배치와 노드 정합성

게이트 B도 먼저 local 두 stage에서 수행한다. 전원 문제를 일으킨 원격 3090
2장 호스트는 사용자가 다시 켜고 전원 안전성을 명시적으로 승인한 뒤에만 쓴다.
local wave가 통과한 뒤 현재 원격 대상인 TUF 노트북 한 대를 추가하고 접속,
3-stage request/placement, TCP boundary, runtime-ready provenance를 검증한다.

1. canonical 40요청 workload를 그대로 실행한다: 20 동시 + 60초마다 5개씩
   4 wave, request당 약 1K input+output budget.
2. 모든 wave가 이전 request의 Decode와 겹쳐야 하며, 각 stage에서 동일한
   physical execution id·row ownership·terminal count를 관측한다.
3. `mixedSamples > 0`만 보지 않는다. Prefill/Decode token·compute time,
   sampled occupancy, queue depth, GPU utilization, VRAM, power를 전체 run 동안
   구간별로 저장한다.
4. 40개 응답 전문을 모두 사람이 읽는다. 40/40 terminal과 40/40 의미 판정이
   함께 통과해야 correctness 성공이다.
5. remote에서 Event 41, stage pipe close, VRAM 상한 초과가 하나라도 발생하면
   즉시 중단한다. 자동 재시도나 power limit 변경으로 실패를 덮지 않는다.

### 게이트 C — 측정 기반 최적화와 P4 경로 승인

게이트 B의 정확한 workload와 seed를 고정한 채 placement, rank 순서,
`batch/ubatch`, pipeline window depth만 한 변수씩 A/B한다. 목적함수는 응답
정확성·VRAM·전원 안전을 제약으로 둔 aggregate Prefill+Decode TPS 최대화다.

- stage별 Prefill/Decode compute time의 긴 stage를 줄이는 split을 선택한다.
- 평균 GPU utilization만 높이지 않는다. wave별 idle gap, batch occupancy,
  aggregate token/s와 tail latency를 함께 본다.
- direct adapter 경로와 P4 relay 경로에 같은 trace를 넣고 response/event
  parity를 비교한다. P4 queue는 bounded transport pressure만 나타내야 하며
  phase scheduling 정책은 없어야 한다.
- parity와 성능 비열화가 확인된 뒤에만 legacy Hop/window inference 경로를
  제거하거나 격리한다.

## 완료 정의

다음 조건이 모두 참일 때만 이번 리팩터링을 완료로 선언한다.

- P4 core의 운영 경로에 Prefill/Decode/UBATCH/rank/KV/model-name 정책이 없다.
- llama.cpp adapter가 단일 요청과 40요청 연속 웨이브를 같은 scheduler로
  처리한다.
- 모든 stage가 정확히 같은 physical plan을 실행하고 terminal 누락·중복이 없다.
- GGUF architecture가 달라도 adapter 수정 없이 stock llama.cpp metadata와
  graph에서 stage memory를 얻거나 명시적으로 fail-closed 한다.
- 최신 source/dist/native provenance가 일치한다.
- 40/40 응답이 terminal, 비어 있지 않음, prompt 합치, 의미 정확성을 모두
  통과한다.
- local 3090 23 GiB, local 4080 12 GiB, TUF 4070 8 GiB 상한과 승인된
  power envelope를 지킨다.
- direct와 P4 relay의 결과가 같고, P4가 성능상 병목이나 phase 정책 소유자가
  아니다.
- aggregate Prefill/Decode TPS, per-session TPS, latency, GPU utilization을 같은
  report에서 재현할 수 있다.

## 관련 문서

| 목적 | 문서 |
| --- | --- |
| 경계가 왜 바뀌었는지에 대한 상세 추론 | [adapter boundary](adapter-boundary.md) |
| stage별 tensor/KV/recurrent 소유권 | [llama.cpp stage memory](llamacpp-stage-memory.md) |
| 현재 native runtime 결정 | [`apps/llama` internals](../../llama/docs/internals.md) |
| canonical workload와 evidence contract | [direct pipeline benchmark](../../../test/benchmarks/direct-pipeline/README.md) |
| P4 전체 구현 지도 | [implementation](implementation.md) |
