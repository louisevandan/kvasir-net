# 초대형 모델 분산 배치 — 현재 상태와 실행 로드맵

최신 현황 정리: 2026-09-12 — 3/3 마감. a43950bed의 독립 생성 묶음/생성 우선 D+P 예산 구현과 로컬1445/0/7·변이6종 검증을 보존했다. MI250 한 호스트8단계에서 생성 전용 raw TPS155.25→206.06,4k 지연 후보 ITL p95 198ms를 확인했다.100k 세 arm은 모두 deadline/정상 응답 수용 실패다. 지연16은 짧은 ITL2220→266ms, 긴 TTFT는 거의 동일하지만250ms 목표·전체 완주는 미달이다. Hy3는 OS 반환 연결 차단으로 BLOCKED다. 엄격한 공통 native 종료 한도도 artifact 복사 후 정리로50.373초 초과했다. 기본값/H5/서비스 승격은 하지 않으며 추가4턴 개발/재시험을 자동 시작하지 않는다.
이 파일은 **현재 목표·상태·작업 순서·단계 승격의 단독 소유자**다.
시험 상세와 실기 판정은 [검증 규약](distributed-batching-verification.md), 계층별 책임/업데이트 격리는
[격리 계약](layer-isolation-contract.md), 기존 문서의 역할은
[문서 안내도](document-map.md)가 소유한다. 최초 문서 이관과 후속 구현을 구분한다.
**현재 상태와 재개 조건은 아래 §0만 우선한다.** §3은 최초 감사, §8의 긴 후속 기록은 시간순 이력이다.
이전 기록 안의 “다음”은 당시의 계획이지 지금 구현을 계속하라는 지시가 아니다.

<a id="current-status"></a>

## 0. 현재 상태 — V1.1 마감 보존, 사용자 후속 지시의 Nemotron LAN 시험 진행

### 0.MiniMax M3 MSA 종결 판정 (2026-09-13)

직전 구형 dense-fallback GGUF의 TPS는 MiniMax M3 MSA 성능 기준선이 아니다. 정상
Bartowski Q5_K_S 8-shard artifact로 CUDA·Metal 이기종 분산을 다시 판정했다. context
4,096, sequence 1, batch/ubatch 128/64, `--flash-attn on --no-kv-unified`를 사용했고,
중앙 호스트에서는 사용자가 지정한 RTX 3090만 사용했다.

native 계획은 중앙 CUDA0, Spark CUDA unified memory, Mac21 Metal의 모든 stage에서
`llama.cpp memory implementation does not declare stage-local residency support`로 exit 7이었다.
실제 CREATE는 4/4였지만 LOAD는 Mac21 native exit 5로 거부됐고 SESSION·질의는 실행되지
않았다. 정상 응답과 TPS는 미측정이다. 상세 증거는
[MSA 분산 적재 거부 기록](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-13-minimax-m3-msa-distributed-rejection.md)이
소유한다.

compat patch 0029는 MSA wrapper에 static flag만 추가해 실제 virtual residency 검사에
영향을 주지 않았으므로 제거했다. 제품은 27-patch fail-closed 상태로 돌아갔다. indexer
누락, flash attention 비활성, 다중 sequence와 unified KV 조합의 명시적 거부는 유지한다.
이 버전에서는 M3 stage residency를 더 수정하거나 재시험하지 않는다.

모델 배치 정책과 사양 수집은 M3 결과와 독립적으로 유지한다. `tools/cluster-inference`는
KV/runtime headroom을 먼저 예약한 뒤 `GDDR -> Mac unified -> GB10 unified -> x86 DDR`
순서의 최소 tier와 service-time 기반 연속 cut을 계산한다. agent INSPECT는 NVIDIA 외에도
Linux AMD DRM과 Apple `system_profiler`를 조사하고, OUTER 수집기는 시각 이력과
`latest.json`을 보존한다. 해당 기능의 fleet 배포·실기 수용은 다음 버전 작업이다.
### 0.Nemotron LAN 혼합 웨이브 후속 시험 (2026-09-12)

사용자의 후속 적재·긴 컨텍스트·혼합 반복 웨이브 지시로 수행하는 별도 실기다. 위 V1.1의
3턴 마감·서비스 미승격 판정을 변경하거나 종료된 알고리즘 개발을 자동 재개하지 않는다.
소스 참조는 `10e8dfc9b`, 실행 어댑터는 `a43950bed` 빌드다. 제품 코드·배치 정책은 수정하지 않았다.

- Nemotron 550B UD-Q5_K_S를 같은 LAN 7대·8단계(CUDA/Metal와 CPU expert offload)에 적재했다.
  실제 LOAD/SESSION 8/8, 짧은 산술 smoke EOS·완료·해제 1/1이다. smoke는 적재를 유지했으므로
  UNLOAD 수용 증거가 아니다. 실행 자료는 `target/nemotron550-all-fleet/`에 있다.
- 기존 cold arm은 새로운 100,038토큰 입력 8건·출력 상한 2,048·resident 8이다.
  2026-09-12 22:11 KST 확인 시 프리필 처리 중이며, 제한 시각은 같은 날 23:28:01 KST다.
  조건을 중간 변경하지 않는다. 장문 정상 응답·전체 해제·UNLOAD는 아직 미판정이다.
- 후속 `target/nemotron550-mixed-waves/`의 실행기를 22:11 KST에 시작했다. 현재 단계는
  `waiting_for_cold_arm`이며 혼합 추론은 아직 시작하지 않았다. 기존 시험과 계측의 종료 및
  기존 native 자식 0개를 확인한 뒤 새 이름·generation의 LOAD/SESSION을 실행한다.
  기존 실패 worker를 정상 drain으로 간주하지 않는다. 장치·배치 정책·KV·resident는 유지한다.
- 입력은 실제 토크나이저 기준 86–6,512토큰 요청과 100,038토큰 요청 1건, 총 127,735토큰이다.
  64건을 8개 웨이브로 0/180/480/780/1080/1380/1680/1980초에 고정 송신한다.
  첫 웨이브의 짧은 입력 8건 중 4건이 긴 출력을 요구하며, 이후 프리필과 생성을 겹치도록 한다.
  출력 요구는 짧은 50·중간 7·긴 7건이다. 실제 소비기의 출력 상한은 공통 2,048이며,
  길이 차이는 프롬프트 요구·EOS·응답별 최소 길이로 확인한다. 서로 다른 hard cap 구현이 아니다.
- 새 LOAD 한도는 75분, 혼합 추론 한도는 별도 2시간이다. timeout 시 해당 실행의 native를
  먼저 수거하고 부분 결과·최초 오류·cleanup 오류를 보존한다. 실행 파일 해시 7대 재확인,
  입력 64건/웨이브/응답 기대값 대응과 context 상한·프롬프트 해시 검사, 실행 스크립트 문법 검사를 했다.
- `MANIFEST.json`은 입력·실행기 78개 파일을 봉인한다. `progress.json`, `mixed-artifact.json`,
  `request-metrics.json`, `report.json`, GPU/RAM 및 실행 로그가 현황·판정 자료다.
  예정/실제 송신 차이 1초 초과는 도착 명세 INVALID로 남긴다. 실제 ITL·TTFT·미완료·D+P
  관측을 분리하며, 의미 검토 전 정상 goodput을 승인하지 않는다.

이는 배치 정책 A/B나 H2/H5 전체 수용이 아니다. FIFO 수용은 그대로이며 긴 요청의 슬롯 예약
정책을 추가하지 않았다. cold와 mixed의 서로 다른 입력 구성으로 TPS 개선율을 주장하지 않는다.
원자료는 현재 로컬 target에만 있으며 장기 증거 번들·최종 결과 기록은 미완이다.
다음 행동은 기존 cold 결과 수거 → 새 LOAD/혼합 웨이브 실행 → 미완료를 포함한 결과와
요청별 대기·생성 지연·장기 프리필 진행의 동시 판정이다.

<a id="v11-plan"></a>

### 0.V1.1 MI250·Hy3 통합 수정계획 (2026-09-11)

실기 증거는 `F:/dev/p4-releases/v11-profiled-cohort-a43950bed-runtime-20260912.zip`에 봉인했다.
SHA256 `89fedfd692dee3f0571fa91737982ee42dd3eafbffce405b9b5d4b1dc6fc0d6a`,682 members 전량 검증.
모델·바이너리 해시 불변, 소유 프로세스/포트 정리 완료, 보호 agent 유지.

**최종 상태가 아래 진행 이력보다 우선한다.** 세 턴을 종료했다. 생성 우선·독립 묶음 코드는 유지하되,
지연16은 다음 버전의 비교 기준 후보로만 둔다. 처리량512는100k에서 짧은 ITL7.346초·완료0/16이므로
서비스 권장값으로 승격하지 않는다.100k 기존/지연은8/16 완료·해제지만 전부length이며 긴8건은미완료다.
Hy3는 실기 미실행/BLOCKED, B2/B3 전체·H5·정상 goodput은 미승인이다.
다음 버전 첫 작업은 timeout 취소/drain/예약 반환과 artifact 전송에 독립적인 종료 watchdog이다.
그 뒤 실제n_kv·CPU mask·HIP attention 비용을 분리해 native 작업 범위를 개선하고 문맥별 사전 프로파일을 만든다.
OS 반환 경로 복구 후 Hy3를 검증한다. 이번 마감에서는 후속 개발을 시작하지 않는다.
[최종 원자료 판정](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#profiled-long-closure).


**최우선 마감 규칙 (사용자 지시, 2026-09-12): 최대3턴.** 이번 지시를 받은 응답이1/3이며,
그 안에 도착한 중간 커밋 요청은 같은 턴의 보강이다. 이후 사용자/자동 goal 재개도 남은 턴을 소비한다.
3턴째에는 테스트 결과·채택/거부·커밋/원격 상태·프로세스 정리까지 마감하고4턴째 개발/재시험을 자동 시작하지 않는다.
이 규칙은 아래 과거의 열린 연구/개발 순서보다 우선한다. 중간의 복원 가능한 지점마다 커밋한다.

| 턴 | 고정 작업 | 종료 조건 |
|---|---|---|
| 1 — 설계 고정 | Sarathi 실제 scheduler/PP 실행기와 기존 원자료 대조. 알고리즘·비용 프로파일 선정 규칙·합격 기준·실기 범위를 고정 | 아래 계약과 증거 §27 커밋 `da6507164`. 완료 |
| 2 — 구현·로컬 검증·실기 시작 | 독립 생성 묶음·생성 우선 잔여 토큰 배정·초기 요청 보호. 실제 소비 반례/변이/전체 시험. 같은 native의 한 차례 비용 프로파일에서 운영값을 확정한 뒤 원격 arm 봉인/시작 | 구현/로컬 검증을 중간 커밋. 측정 중 소스/프로파일 변경 금지 |
| 3 — 실기 마감 | MI250/Hy3의 선언된 대조·긴 입력/출력 완료 또는 명시적 제한시간 실패 판정. 응답 전문·지연·처리량·메모리·정리 검산 | 권장값 또는 기준 유지 판정, 증거/코드 커밋·push 확인. 후속 턴/탐색 없음 |

**현재2/3 — 구현 중간 지점:** 생성 인구 기반 독립 묶음, 초기 prefill 폭 보존, 합계 토큰 상한,
합법적 생성 폭 이하의 coalescing 목표를 구현했다. 어댑터557/0/0, composer4/0을 통과했다.
최종 fixture를 이전 소스에 이식하면 실제 소비 시험2개가 실패한다. 구현 중간 커밋은 `a9d2d1a63`이다.
후속 검증은 전체 워크스페이스1445 passed/0 failed/7 ignored(58 summaries), 독립 변이6종 모두 각1실패,
docs-lint94 clean으로 종료했다. 소스와 각 재컴파일 바이너리 해시는 증거 §28에 결속한다.
소스 `a43950bed`를 main/GitHub main에 push하고 MI250/Spark/Ubuntu/Mac 및 Windows Release 빌드를 완료했다.
한 차례 사전 프로파일6건은96/96 완료·해제·UNLOAD 통과다. 지연 후보 합계16행, 처리량 후보512행을
선정 규칙대로 봉인했다. 이는 교정 표본 내 선택이며 성능/서비스 승인이 아니다. 같은 소스·native의
생성 전용 대조를 시작했으며 이후4k 혼합 대조와 실제100034토큰×8 긴 요청+짧은8요청을 실행한다.
출력2048/최소1024/EOS, context102400을 고정했다. 공통 실기 cutoff는2026-09-12 05:25:14 KST다.
Hy3는 새 시험 exe의 로컬 Windows TCP 차단으로 P4 반환이 실패했고 규칙 변경은 관리자 권한 부족으로
거부됐다. 관리자용 범위 제한/원복 스크립트와 사용자 입력 요청을 준비했다. 무응답은 허용으로 해석하지 않는다.
원격 시험 agent와 임시 원격 규칙은 정리했다. 다음3/3에서 남은 실기·판정·증거 번들·정리를 마감한다.

**3/3 진행 — 짧은 대조 종료:** 생성 전용155.25→206.06 raw TPS, pooled 실제 ITL p95 92→89ms다.
4k 혼합 기존75.36/76.08 TPS 대비 처리량 후보83.11 TPS·짧은 ITL p95 2550.5ms·긴 TTFT p50 16.715s,
지연 후보56.19 TPS·198ms·39.031s다. 초기 짧은 TTFT 회귀 기준은 두 후보 모두 통과했다.
이6개 holdout은96/96 EOS·완료·해제·UNLOAD이며120–180단어 계약은 전량 통과하지 않아 정상 goodput
승인은 아니다. 실제100k 대조는 실행 중이다. 현재 자료로 기본값·H5·Hy3 승격을 하지 않는다.

**3/3 실행 예외 기록:** 100k 기존 대조는1800초에8/16 완료·해제로 실패했고 UNLOAD busy로 native 강제 정리가 필요했다. 원래 runner는 예정대로 중단했다. 이후 소유 프로세스 부재·9개 포트 반환·8 GPU 유휴/512MiB 미만·기존 보호 agent 생존을 별도 확인했다. 이 자원 감사 통과를 조건으로 이미 봉인한 Q512/Q16의 두 긴 arm만 원래1800초/공통05:25:14 KST 한도에서 실행한다. 자동 중단 원문은 보존하며 코드·프로파일·입력·출력·합격선은 바꾸지 않는다. 이는 실행 순서의 명시적 예외이며 실패 대조를 승인하지 않는다.

**이번에 구현할 정책:** Sarathi-Serve의 사전 프로파일 기반 token budget + 생성 우선 chunked-prefill을
P4의 유한 독립 flight에 적용한다. P4 고유 부분은 admitted ready/inflight 인구로 요청 묶음을 보존하는 것이다.
전량 decode 합류와 요청을 처리하면서1행부터 배우는 기존 cold 시간 게이트는 이번 운영 후보에서 사용하지 않는다.
방식은 하나이며 OUTER가 **처리량 우선/응답 지연 우선** 목적에 맞는 사전 확정 프로파일을 선택한다.

- 생성 참여 상한은 순간 ready/free가 아닌 전체 활성 생성 인구와 독립 묶음 목표에서 구한다.
  기본 안전 묶음 목표는 기존 허용 flight 창 안이다. 노드 실행1·요청당 decode outstanding1은 유지한다.
- 선택한 묶음의 생성 행부터 예약하고 남는 **합계 토큰 예산**에 P를 배정한다. 큰 요청 하나가 전체
  ready 집단을 소진하지 않으며 기존 회전 순서와 prefill 진행 기회를 보존한다. 서로 다른 단계의 합법적인
  진행을 전역 drain으로 막지 않는다. coalescing 목표가 실제 묶음의 수용 폭보다 커지지 않게 한다.
- 실제 생성이 아직 시작되지 않은 초기 P 묶음을 '미래 생성' 하나 때문에 작은 quantum으로 바꾸지 않는다.
  초기 짧은8건의 TTFT 회귀 반례를 포함한다. 비용 학습/준비 비용은 별도 측정하고 첫 고객 요청에 숨기지 않는다.
- 비용 프로파일은 모델/native/장치 배정/phase/문맥/폭에 결속한다. 이미 있는 동일 조건 표본을 우선 사용하고
  부족한 비용 표본은 사전 고정한 한 번의 프로파일 절차로 채운다. 본 시험의 결과를 보고 설정/판정선을 바꾸지 않는다.
  처리량 우선은 측정 범위에서 유효 처리량을 최대화하고, 지연 우선은 지연 목표 안의 가장 큰 합법적 토큰
  예산을 선택한다. MI250250ms·Hy35000ms는 이전 수치에 결속한 **실험용 실제 ITL p95 목표**이며 사용자 제품 SLO의 확정은 아니다.
- 미측정100k 비용을4k의 선형 외삽으로 승인하지 않는다. 프로파일 범위 밖은 고정된 안전 한도와 실제
  측정으로 판정하고, 지연 목표 충족 여부를 미리 보증하지 않는다. 기존 resident/open/fragment 한도를 늘리지 않는다.

**판정 고정:** 현재26c13b9ae 기준의 같은 native/모델/배정/입력과 비교한다. 생성 전용 처리량≥기준95%,
실제 ITL p95≤기준110%, 초기 짧은 요청 TTFT p95≤기준125%를 두 운영 모드 공통 회귀 기준으로 둔다.
이는 이번 엔지니어링 선별 기준이며 H5의 승인 기준을 대체하지 않는다. 지연 모드는 위 실제 ITL 목표,
처리량 모드는 같은 부하/품질 계약의 유효 처리량을 평가하고, P 처리량/긴 TTFT 손실을 숨기지 않는다.
부하 진단용 고정 출력 길이와 자연 EOS/내용 검증을 구분한다. 기존120–180단어 실패는 그대로 기록하며 judge를 완화하지 않는다.
생성 전용/4k 혼합/실제100k 긴 입력·긴 출력/완료→해제→UNLOAD가 선언 범위다. 클러스터 내 대조를 우선하며
MI250 한 호스트 결과를 Hy3 수용으로 대체하지 않는다. 원격 실기 전체에는2시간 공통 cutoff를 적용한다.
호스트 접근/메모리/예산이 막힌 경우 원문 실패와 부분 결과를 남기고 이번3턴 안에서 판정을 종료한다.
안전성 결함의 수정·재검증은 허용하지만 성능 결과가 마음에 들지 않는다는 이유의 후보 재설계/knob 탐색은 하지 않는다.
[원문/실행기 대조와 선택 근거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#three-turn-closure)가 상세다.

**사용자 재개 목표 (2026-09-11, 기준 `5ee827af5`):** main/원격 일치 확인 → 최신 공식
llama.cpp 호환·Release 빌드·소형 모델 → 검증된 배치 설계를 실제 소비 경로에 구현 → MI250와
Hy3 재실기를 하나의 작업으로 수행한다. 기존 두 증거 ZIP은 불변 기준선으로 보존한다.
로컬 main과 GitHub main은 시작 시 같은 전체 SHA였고 dirty/untracked는 없었다.
최신 upstream 발견값은 `451b89bae0c4b1dd612eb503ceace906c01ddcc9`이며, 채택 승인과 구분한다.

이번 구현은 decode 묶음 knob의 추가 탐색으로 끝내지 않는다. 아래 계약으로 진행한다.

1. **독립 prefill 묶음:** MI250의 초기 pure-prefill 91개는 모두 16요청×32행=512행,
   발행 전 open=0이었다. 후보/기준 양쪽에서 같다. 전체 폭을 유지하면서 참여 요청 수를
   제한하여 다른 요청 묶음을 즉시 발행 가능하게 한다. fragment=1에서 먼저 검증한다.
   16요청에서 2요청×256행은 8개 독립 묶음을 만드는 첫 반례 입력이지 최적값이 아니다.
2. **phase별 비용과 공정성:** pure-prefill은 효율적인 chunk를 채우고, decode와 공존하면
   별도 prefill 서비스 예산을 적용한다. 짧은 prompt/새 요청을 영구 대기시키지 않으며
   선택 거부는 fairness를 소비하지 않는다. prefix 길이·phase·묶음 크기가 다른 RPC 평균을
   하나의 고정비로 회귀하지 않는다. 시간 예산은 비선점 native 호출의 실시간 보장이 아니다.
3. **안전 예산:** pending prompt/반환/flight와 broker receipt를 각각 제한한다.
   원본이 필요한 duplicate/replay 계약을 payload 삭제나 해시 동등성으로 몰래 바꾸지 않는다.
   node 실행1·decode outstanding≤1·KV 완료 후 재사용은 유지한다. fragment 창 증가는 별도 증명 뒤다.
4. **연구 결속:** 최신 llama.cpp `tools/server/server-context.cpp::update_slots`,
   vLLM V1 `scheduler.py`, SGLang `schedule_policy.py::PrefillAdder`, Sarathi-Serve를 비교한다.
   토큰 예산·KV 예약·chunked prefill은 재사용할 설계 원리이며, 단일 인스턴스의 전체 decode
   일괄 선택을 분산 파이프라인에 그대로 이식하지 않는다. 미검증 GPU 비용을 최적값으로 선언하지 않는다.
5. **검증:** 각 수정은 실제 소비 반례→수정→독립 변이→전체 게이트로 결속한다.
   각 클러스터 안에서 같은 native 위의 이전 정책/새 정책을 비교해 upstream 갱신과 정책 효과를 분리한다.
   MI250는 새 451 native, Hy3는 기존 fleet 434 native를 양 arm에 고정한다. 최신 native의 전 플랫폼
   채택은 이번 policy 비교와 별도이며 Hy3 전부를 최신 upstream으로 재검증했다고 표시하지 않는다.
   작은 판별 arm 이후 실제 긴 입력 다수와 긴 출력을 실행하며 입력 토큰 수를 tokenizer로 확인한다.
   100k 입력에 출력 예산을 더한 context, KV 우선 device 배치, RAM offload 및 host CPU 예산을 명시한다.
   MI250 타 작업 점유는 감시하며 무관 프로세스를 종료하지 않는다. Hy3의 CPU expert 병목도 따로 판정한다.

이 재개 항목이 현재 첫 행동을 정한다. 아래 이전 진행표의 미완 예산/품질 게이트는 그대로 열린다.

**재개 진행:** 최신 pin의 26개 패치가 clean replay/분류 검사를 통과했다(`0cf373d83`).
CUDA sm_86 Release CTest16/16, MI250 gfx90a ROCm Release CTest15/15 및 Qwen2.5-1.5B
실제 CUDA 추론·KV 저장/복원·UNLOAD 1/1을 완료했다. 소형 단일 장치 승인 범위다.
수용 입력 계정은 pending/active/공유 provenance의 count·capacity bytes·입력/출력 토큰을 제한하도록
실제 PREFILL 소비 경로에 연결했다. 이것은 B2/B3 전체가 아니며 native 반환/outbox/broker/remote grant는 남아 있다.
수용 입력 단위는 workspace **1391 passed / 0 failed / 7 ignored**(58 summaries), 실제 소비 시험2개,
독립 변이(수용 우회1실패·퇴역 누락2실패)를 확인했다(`54b4fd9b5`, main push).
선택적 pipeline policy는 eligible 요청 수를 남은 flight slot 수로 나눠 pure-prefill의 전체 행 폭을
유지하면서 독립 묶음을 만든다. decode가 비행 중인 동안도 mixed-prefill quantum을 적용한다.
finite open window·fragment1만 허용하며 기본 비활성이다. 비용 모델/시간 SLO/B2·B3 전체는 아직 아니다.
실제 loop 반례와 독립 변이2종(각2실패), workspace **1396/0/7**(58 summaries, exit0),
공통 OUTER composer의 실제 CLI 거부/보존 포함 Node4/4를 확인했다. e4503e10f로 양 클러스터의 같은 조건 대조 실기를 마감했다.

**이번 재개 마감·현재 판정:** 독립 prefill 묶음은 실제로 반환 대기를 줄였다. MI 초기 중앙512행을
유지하면서 참여16→2요청, head RPC 사이 대기1948→196ms, 전체 동시 RPC0.96→4.58/8이 됐다.
MI raw generated22.83→39.00TPS(+70.83%), 모두 첫1024 생성1060.5→394.9s지만 후보EOS15/16·선두 계산2/16으로
정상 응답 성능은 미승인이다. 동일16 prompt의 P4 없는 native 기준도 계산2/16으로 모델 경로 자체의 오류를 재현했다.
Hy3 후보는8/8 EOS·완료·해제·UNLOAD,7.5298TPS·선두 계산7/8이다. 기준은2시간 공통 cutoff에서0/8 완료로
부분3164출력을 보존했다. 모두 첫 출력1871.1→960.4s만 공통 지연 비교이며 전체 TPS 개선율은 계산하지 않는다.
실제 최장 입력약10.6k이며100k 입력·연속 웨이브·H5·최적 정책은 검증하지 않았다. 기본 비활성은 유지한다.
원자료·실행파일132경로의 전후 해시·정리 및 인과 분석은
[독립 prefill 대조 실기](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#prefill-cohorts-20260911)에 있다.

**현재 다음 첫 행동:** 아래 c6bd6c597 후보의 전량 decode 합류를 성능 정책으로 채택하지 않는다.
prefill이 없어도 독립 생성 묶음을 유지하도록 ready/inflight 인구와 측정한 stage 비용을 분리한다.
16개 생성 요청·창8의 실제 소비 반례에서 첫 tail 반환 전에 여러 독립 flight가 발행되고,
반환을 반복해도 한두 묶음으로 합쳐지지 않는지 먼저 고정한다. 선택된 묶음 안에서는 생성 우선을 유지한다.
그 뒤 같은 native/입력/배정의 생성 전용·혼합 대조에서 생성 지연과 처리량의 무회귀를 확인한다.
시간250ms 후보도 권장하지 않는다. 임의의 더 큰 숫자로 통과시키지 않고, 초기 학습 지연과 실제 실행 잔여,
전송/반환 및 요청 deadline을 포함한 비용 선택, prefill 시간 deficit/aging을 차례로 결속한다.
실제100k 확대는 독립 receipt/미래 반환 byte 예산과 이 소비/실기 게이트 뒤다.
Hy3는 추론 전 Mac agent의 역방향 P4 응답 `No route to host`에서 중단했다. Python TCP 성공을
agent 통신 승인으로 확대하지 않는다. 시험 프로세스 정리·M42 한정 방화벽 예외 복원을 완료했다.
[실기 범위·반증·RPC 귀속](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#cohort-runtime-audit)을 따른다.
[소비 반례와 검증](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#prefill-population)을 따른다.

**생성/시간 정책 구현 (2026-09-12, 로컬 GREEN):** 순간 빈 flight로 ready decode를 나누지 않고,
명시 상한/물리 용량을 지키며 먼저 배정한다. 마지막 prefill 반환도 미래 생성 서비스로 고려한다.
decode-only 비용 피드백과 open 작업의 순서를 포함해 마지막 stage RPC 완료를 예측하고,
큰 후보를 거부하면 prefill 행을 줄여 실제 계획을 다시 만든다. 모르는/불가능한 비용은 이전 prefill이
없을 때1행 probe로 측정한다. 기본 비활성·ordinary/fragment1·기존 창을 유지하며, 이전 stage별
서비스 예산과 같은 숫자가 같은 의미는 아니다. 전체1439/0/7(58 summaries, exit0), 독립 변이5종
각0pass/1fail, 타이머의 실제 대기/무입력 재발행/원장 불변을 확인했다.
초기 작은 shape를 학습하려면 큰 prefill의 반환을 먼저 기다려야 하는 장벽도 실제 소비 반례로 고쳤다.
큰 작업 뒤 기존 창 안의 cold1행 probe 하나를 허용하되, 이미1행 probe/정체불명 open이 있으면 추가하지 않는다.
보강 후 전체1441/0/7·독립 재컴파일 변이6종 각0pass/1fail을 확인했다.
전송·dispatch·반환 잔여까지 포함한 client deadline, 시간 deficit/aging, 전체 반환 예약은 아직 남는다.
[구현·반례·검증 범위](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#generation-service-policy)가 기준이다.

**생성/시간 후보 실기 판정 (c6bd6c597, 성능 RED):** 같은 MI250 한 호스트8stage·새 HIP native에서
기존26c13b9ae/생성 우선/생성 우선+시간250ms를 A/B/C/C/B/A로 비교했다. 각16/16 EOS·완료·해제·UNLOAD,
45층 ROCm 실제 배정과 실행파일 전후 해시가 통과했다. 실제 입력은213토큰8개+4255토큰8개이며100k가 아니다.
raw TPS는 기존72.84–75.04, 생성 우선50.27–52.31, 시간 포함30.43–32.53이다. 전체 분모·출력량은 증거에 있다.
긴 prefill 중 짧은 요청의 실제 ITL p50은616–628→646.5–680→172–180ms지만,
긴 요청 TTFT p50은21.09–21.21→22.23–23.59→65.16–69.92초다. 시간 후보의 초기 짧은7요청도
첫 출력이10–13초로 늦어졌다. 순수 생성·활성16 조건에서 생성 우선은 폭2→약8, 발행 전 open6→약1,
head RPC 사이 대기0.51–0.55→71.82–77.69ms로 회귀했다. 생성 우선과 전량 합류를 동일시한 설계를 정정한다.
120–180단어 지시는 전체96건 중50건만 충족했다. EOS/기본 judge 통과를 정상 goodput 승인으로 확대하지 않는다.
시험 프로세스는 정리했고 기존 MI agent43015는 보존했다. 기본 비활성 유지·권장 설정/최적값 승격 거부다.
[6arm·회귀 원인·수정 계약](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#generation-service-screen)이 현재 판정이다.

**사용자 제안의 조사 결론:** 생성 우선 후 남는 예산에 prefill을 넣는 방향을 채택한다. P4 일반 attention은
이미 생성 행 우선이나 혼합 결과는 전체 계산 뒤 반환하며, 고정128행 quantum은 생성 지연을 보호하지 못했다.
동일 GPU 배정 실기의 짧은 요청 token 간격 p50은 긴 요청 유입 전45ms/긴 prefill 중637.5ms/이후76ms다.
다른 대조도46/636.5/76ms다. 구간별 문맥과 수요가 달라 인과 배율은 주장하지 않는다.
Sarathi-Serve·vLLM·DeepSpeed-MII·TensorRT-LLM·SGLang·현재 llama pin을 코드/원문으로 대조했다.
[조사·코드 결함·판별 실험](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#decode-first-research)이 상세 근거다.
이후 순서는 (1) 독립 생성 묶음의 반복 반환 무회귀와 ready→issue/tail·stage별 잔여 계측,
(2) 관측한 비용으로 묶음 폭/동시 flight/청크를 공동 선택하고 초기 학습의 장기 대기를 제한,
(3) 생성 우선 후 문맥·기발행 backlog·전송을 포함한 요청별 시간 여유 안의 chunk 재선택,
(4) prefill 시간 deficit/aging과 전체 KV/반환 byte 예약을 결속한 실제100k 다수/긴 출력 수용이다.
pure-prefill에는 충분한 행 폭과 독립 집단을 유지하고, 병목이 이미 바쁘면 flight를 더 쌓지 않는다.
equal-width hybrid는 합법적인 phase 분할로 같은 서비스 목표를 구현한다. 기본 승격/최적값은 실기 뒤다.

이번 native는 `--expect-layer-device begin:end:name`을 no-alloc PLAN과 실제 LOAD에서 대조하며,
byte 총합이 같아도 PLAN/LOAD의 기본 레이어 장치가 바뀌면 거부한다. 명시 CPU 범위/전문가 오프로딩은 구분한다.
workspace1430/0/7, CUDA CTest16/16, 실제stdin7/7, 독립 재컴파일 변이5종 각각1실패,
제품agent2개·GPU UUID2개·Qwen1.5B4/4 EOS·완료·해제·UNLOAD를 확인했다. 품질 지시 전체/100k 승인은 아니다.
한 native가2GPU를 자동 분할할 때 compute PLAN/실제12MiB 차이는 신구 바이너리 모두에서 재현한 별도 RED로 남긴다.
[반례·소비 검증·제약](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#layer-placement-gate)을 따른다.

MI native451의 기존 OUTER 계획 `45-cut_begin`은 각 stage의 첫 반복층을 CPU로 배정했다.
배정만 정정한 raw27.42→75.02TPS는 배치 알고리즘만의 성과가 아니다. 정정된 계획을 다음 MI 기준선으로
고정하되 다른 native pin/Hy3에 숫자+1을 일괄 적용하지 않는다. 새query의 ROCm PLAN/잘못된 LOAD 거부와
올바른 LOAD/추론은 확인했다. 같은 새 native의 Metal 실기는 아직 남아 있다.

이전 CPU 배정에서 독립 prefill 최대2건은 head 유휴670→132ms·raw14.86→27.42TPS를 만들었지만
혼합 ITL863→1078ms가 됐다. 정정된 GPU 배정에서는 자동 정책도 이미1개 prefill 요청씩 발행해
상한2 추가 효과가 없었다(74.70/75.02TPS). 따라서 모델명별2건 상수를 기본값으로 넣지 않는다.
다음 정책 단위는 phase별 active/ready/inflight 인구와 stage 비용을 보고 독립 묶음·행 폭·flight 목표를
분리하는 것이다. 느린/offload 경로의8요청 동시 outstanding 반례와 GPU 경로의 기존 공급 무회귀를 함께 검증한다.

고정250ms 서비스 예산은 최대2건 조건에서 raw27.42→13.87TPS·긴 TTFT72.813→205.312초로 악화해
권장/기본 정책으로 채택하지 않는다. 시간 피드백은 유지하고, 실행 잔여·기발행 작업·전송을 반영한
decode tail 완료시각 아래 discrete chunk와 coalescing을 고르는 정책으로 발전시킨다. prefill deficit/aging도 결속한다.
기존 안전 창은 먼저 유지하며, 실제100k 입력 확대 전 exact broker receipt/미래 반환 byte 선예약을 닫는다.
실제100k 다수 도착/생성 중 유입/1–2개 긴 요청·긴 자연 EOS와 내용 수용을 별도로 반복한다.
V1.1-3 fragment 창 확대는 KV 순서·취소/반환 증명 뒤이며 H5·최적 정책·정상 goodput은 여전히 미승인이다.

Hy3 새5호스트 agent 빌드는 끝났지만 이번 연결 검사는 M42 새 실행파일의 TCP 차단에서 멈췄다.
M42 임시 규칙은 복원했으며 로컬 시험 실행파일 TCP51118은 관리자 권한 부족으로 변경하지 못했다.
필요한 로컬 허용을 사용자에게 전달했고, 허용 뒤 같은 fleet의 추론을 재개한다. CPU 전문가/DRAM 병목은
KV 우선 조건 아래 cut/weight 배치도 함께 재계획한다. MI6arm을 Hy3/다중 물리 호스트 승인으로 확대하지 않는다.
[원격6arm·인과 분리·다음 구현](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#service-screen-placement)을 따른다.
아래 실기 수치와 당시의 “다음”은 이력이며 이 현재 순서를 대체하지 않는다.

**고정 창 안의 발행 결함 우선 수정 (2026-09-11, 로컬 검증 완료):** 반환 예산의 전체 연결은 유지하되,
이미 허용된 pipeline policy의 프리필 요청 수 오판과 끝없는 decode coalescing을 먼저 수정한다.
이는 새로운 resident/open/fragment 상한을 허용하지 않으며 B2/B3 완료나 시간 기반 비용 정책 승격이 아니다.
ordinary attention의 실험 정책에서 prefill 계획은 decode 최소 요청 수로 막지 않고, decode-only 묶음의
대기는 단조시계로 제한한다. 만료는 실제 Worker 수신 대기를 깨우되 native/KV/flight 권한을 대신하지 않는다.
기존 정책·atomic/equal 경로는 보존한다. 실제 Worker loop와 capacity1 completion에서 마지막 독립
prefill 발행, 새 입력 없이 decode 대기 만료, 기존 상한·정산·해제를 검증한다. 그 뒤 반환 예산 연결을 계속하며
flight 확대와 누적 prefill 시간 정책·실제100k 승인은 계속 열린다. workspace1404/0/7(58 summaries, exit0),
독립 변이2종의 실제 소비 실패2/1, docs-lint94 clean을 확인했다. decode-only 대기는2ms 뒤 발행 자격을
재검사하며 native/OS 지연의 상한은 아니다. [반례·검증·봉인](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#phase-pacing)을 따른다.

**소유형 반환 전달 경계 (2026-09-11, 로컬 검증 완료):** `RetainedEventBroker`와 `RetainedEventNode`를
명시 소유형 adapter 경계에 연결했다. 실제 queue1 소비에서 동시 held input/output, 독립 front 진행,
Full·닫힘·중복·등록 변경의 원본/claim 보존과 반복32회 퇴역을 검사한다. 목적지 queue slot과
retained bytes를 함께 예약하며 receiver dequeue는 byte 반환이 아니다. receipt는 독립 exact 사본이다.
제품 composition root/llamacpp Worker/connection writer와 receipt byte 상한은 아직 raw/미연결이며
이번 경계 구현을 B2/B3 완료나 GPU 성능 승인으로 표시하지 않는다. workspace1414/0/7(58 summaries,
exit0), 독립 재컴파일 변이3종7/1/1실패, docs-lint94 clean이다. 다음은 같은 소유형 경계를 Worker와
control/connection writer에 실제 연결하고 필수 반환/독립 receipt 선예약을 닫는 것이다.
[계획·소비 시험·판정](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#retained-broker-node)을 따른다.

**실제 llama.cpp Worker 소유형 연결 (2026-09-11, 로컬 검증 완료):** 같은 `Worker::run`의 공통 loop를
소유형 adapter에 연결했다. 현재 입력·Full 중 보류 입력·지연 ACK 원문을 claim과 함께 유지하고,
중단 후 원문/미처리 receiver/state/effect는 adapter owner에 남긴다. 실제 broker→EventNode→
llama adapter→Worker 두 단계 경로에서 기존 `ordinary-2` 골든과 네 요청 웨이브·해제·UNLOAD를 검사한다.
native 계산만 대체한 로컬 소비 시험이며 GPU/원격 실행이 아니다. 제품 root/control/writer·미래 반환 및
독립 receipt byte 예약은 아직 남아 있다. workspace1416/0/7(58 summaries, exit0), 실제 소비2시험,
독립 재컴파일 변이3종 각각1실패, docs-lint94 clean이다. [소비 반례·검증](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#retained-worker)을 따른다.

**장기 다중 prefill에 대한 설계 결론:** 배치 행 폭·독립 요청 묶음·동시 flight 목표를 각각 선택한다.
pure-prefill은 효율적인 큰 행 폭을 유지하면서 다른 요청을 다음 발행에 남긴다. decode가 시작되면
각 stage 앞에 이미 쌓인 작업까지 포함한 예상 완료시각으로 prefill 추가량을 제한하고, prefill에는
누적 서비스 deficit/aging과 과부하 수용 제한을 적용한다. 느린 stage가 계속 바쁜 경우에는 flight를
더 쌓지 않고 KV 우선 조건 아래 컷/weight offload를 재계획한다. 이 전체 비용 정책은 아직 구현되지 않았으며,
첫 단위인 stage별 prefill 서비스 backlog 예산을 아래에서 별도 검증한다.
반환 수명 연결 뒤 비용 계측→고정 창 안의 정책 소비 반례→같은 실제100k 입력/긴 출력의 대조 순서다.
[원인·결정식·실험 판정](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#long-prefill-design)을 따르며, 기존 실행을100k 또는 GPU 포화 승인으로 바꾸지 않는다.

**제품 event runtime 소유형 연결 (2026-09-11, 로컬 검증 완료):** entrypoint의 root/control/TCP가
소유형 broker/node/adapter를 실제 선택한다. 제어 Full은 같은 원본을 재시도하고, 영구 실패는
입력·응답·미처리 입력·노드 owner를 보존한다. 소켓 쓰기 실패는 현재 원본과 아직 쓰지 않은 큐를
보존하며 자동 replay하지 않는다. 삭제는 ingress를 잠시 막고 입력/완료의 retained count0을 확인한다.
원격 ACK·원시 frame/직렬화 scratch·미래 native/effect/receipt byte 예약은 여전히 남는다.
workspace1424/0/7(58 summaries, exit0), 독립 재컴파일 변이6종1/2/1/1/1/1실패, docs-lint94 clean이다.
실제 제품 agent→native Qwen1.5B CPU 2-stage에서 resident2/2wave의4요청 EOS·완료·해제·UNLOAD/DELETE를
확인했다. native451·모델·실행 agent/driver 해시와 원문 응답을 봉인했다. GPU/100k/문장수 지시 전체 승인은 아니다.
이번 연결 뒤 다음 첫 행동은 그 선예약과 전체 반환 수명을 닫는 것이다. 이어 고정 창 안의
누적 prefill 시간 정책·실제100k·두 클러스터 대조로 진행한다.
[실제 TCP·실패 반례·검증](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#retained-runtime)을 따른다.

**stage별 prefill 서비스 예산 (2026-09-11, 로컬 검증 완료):** 기존 유한 창에서 실제 stage Frame 시간 피드백을
head 선택에 연결했다. 미완료 prefill의 stage별 예상 비용+후보 비용이 예산을 넘으면 ready decode만
다시 계획한다. pure-prefill 폭은 유지하고, 표본 부재는 cold, 앞선 prefill 정산 뒤 최소 서비스는
progress probe로 드러낸다. 기본 비활성이고 전체 decode deadline·동적 행 폭·요청별 시간 aging이나
B2/B3 선예약의 완성은 아니다. workspace1430/0/7(58 summaries, exit0), 독립 변이5종 각1실패,
실제 제품 TCP agent2개/CPU native2stage에서4/4 EOS·해제·UNLOAD/DELETE를 봉인했다. 비용 초과16건은
실제 prefill0/decode≥1로 발행됐다. 의도적 service1ms 기능 시험으로 성능 개선율이나 권장값은 아니다.
최초 전체 시험의1429/1/7은 기존 fixture의 호출 진입/완료 경쟁이었고 기대값을 유지한 완료 대기로 고쳤다.
이후 새 비용 정책을 실제 두 클러스터의 고정 창 대조로 선별하고 cold/예측오차를 확인하여 chunk 선택과
전체 완료시각 예측으로 확장한다. 추가 자원 허용 전 반환 선예약을 닫는 조건은 유지한다.
[결정·반례·검증](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#stage-service-budget)을 따른다.

**반환 수명 계측 진행:** broker의 exact Event 사본을 indexed/retired/allocated로 구분하고
실제 INSPECT 제어 응답에 바이트·퇴역·최종 해제량을 연결했다. 큐 dequeue와 원장 퇴역, 마지막
참조 해제를 별도로 관측한다. 일반/예약 completion dispatch 및 실제 control loop 반례를 따른다.
workspace1401/0/7·독립 변이2종 각2실패, 실제 로컬 TCP에서64MiB 처리 뒤 남은 payload67112267B의
송수신 원문 대조를 완료했다. GPU 실기와 B2/B3 완료를 뜻하지 않는다.
이는 V1.1-0 중 receipt 관측 단위이며 B2/B3 byte 제한이나 조기 삭제가 아니다. 다음은 이 값으로
실제 retained 저장소를 귀속하고 actor queue/반환 예약을 연결하는 것이다. 검증 결과는
[receipt 수명 계측](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#receipt-memory-observation)을 따른다.

**단일 main 운영 (2026-09-11 사용자 지시):** 장기 개발·릴리즈 기준은 `main` 하나다. 모델별 개발 브랜치는 운영하지 않는다.
merge `429e057de`로 임시 Hy3의 upstream/메모리/physical-wire 호환 변경과 main의 하드웨어 조회·경로·Linux 링크 수정을 통합하고 push했다.
모델·클러스터·정책·워크로드·runtime identity는 `test/benchmarks/cluster-inference/` 공통 구성기의 독립 설정이다. 배포/lifecycle/deadline runner의 완전한 공통화는 후속 작업이다.
과거 측정 소스/바이너리 해시는 불변 증거로 보관하며, 통합 소스로 재배포하지 않은 실기를 통합 main의 승인으로 표시하지 않는다.
통합 게이트와 push 후 병합된 임시 branch ref를 정리했다. 로컬·GitHub에는 main만 남고, 기존 worktree는 동일 commit의 detached 상태로 보존했다. 이후 실험 결과와 수정은 main에 반영한다.

**구현 진행 (2026-09-11):** V1.1-0의 선택 시점 진단·실제 OUTPUT 수신시각과 V1.1-2의
실험용 phase별 요청/행 상한을 구현했다. V1.1-0 전체 또는 V1.1-2 승격 완료는 아니다.
V1.1-1 B2/B3/receipt 예산은 미완이며 resident/open/fragment 창을 확대하지 않았다.
새 정책은 기본 비활성이고 ordinary attention에만 적용한다. 두 클러스터 GPU 실기 선별을 수행했으며 H5 성능/서비스 승인은 미완료다.
배치 구현 당시 게이트는 workspace1384/0/7·변이 실패1/5/3/1이다. 통합 main 게이트는 **1389/0/7**(58 summary), Node21/21, CPU Release CTest15/15, docs-lint94 clean이다. CPU 게이트를 GPU 승인으로 확대하지 않는다.
[구현·검증 기록](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#bounded-implementation)을 따른다.
다음은 V1.1-0의 미발행 구간/요청별 사유·단조시계 issue→settle·byte 수명 계측을 닫고 V1.1-1을 수행하는 것이다.
그 전에는 이번 선택 후보를 기본 정책으로 승격하거나 flight 창을 늘리지 않는다.

**동시 실기 선별 (2026-09-11):** MI250 두 호스트/16stage와 Hy3 다섯 호스트/6stage의 실제 추론을 병행했다.
Hy3는 decode cap0→2에서 **5.32→9.55TPS**, ITL p50 **1.179→0.533s**, 양쪽8/8 완료·해제·UNLOAD였다.
MI250는 기본 CPU 설정에서 cap4 실패·cap8 8.35TPS(기준28.54/29.73)로 회귀했다. cap4/min4도 폭은 안정됐지만 실패했다.
native CPU threads4를 쓴 cap4/min4 후보는 **34.05TPS**, ITL p50 **0.319s**,
16/16 완료·해제·UNLOAD였다. 같은 CPU4 기준군은13.59TPS로 완주했지만 외부 Python GPU 작업이
겹쳐 **배치 단독 개선율 비교에서 제외**한다. GPU 비간섭 시간대의 재검증 전 개선율 승격은 BLOCKED다.
Hy3도 한 쌍의 선별 결과다. context100k/출력256의 짧은 부하이며 실제100k prefill·정상 EOS·연속 웨이브 승인이 아니다.
MI 소스d5256af44, Hy3는 fleet 호환을 유지한b9deee4ce와 동일한 기존 Mac downstream agent를 사용했다.
[원자료·수치·실패·해시](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#dual-cluster-screening)를 따른다.

**긴 입력·긴 출력 실기 (동일 날짜, 별도 워크로드):** 사용자 2시간 한도 안에 Hy3 5호스트/6stage와
비점유 MI250-B 1호스트/8stage를 병행했다. Hy3는18809생성/3834.099s=4.9057TPS,
8/8 EOS·완료·해제·UNLOAD 및 선두 계산8/8이다. MI cap4 후보는51990생성/1315.775s=39.5128TPS,
16/16 완료·해제·UNLOAD, EOS14/length2, 선두 계산2/16이다. 두 모델 모두 전체 응답 품질은 미승인이다.
context는100k지만 최장 실제 입력은Hy3 42154/MI42413토큰이며 실제100k prefill을 완료하지 않았다.
Hy3 TTFT max37.13분, MI 후보 agent peak RSS30.934GiB 및 UNLOAD 뒤30.645GiB 잔류를 확인했다.
MI 후보의 모든 첫 출력 전/후 ITL p50은2.669/0.203s로 장기 prefill의 영향이 크다.
동일 MI250-B cap0 기준은 공통 cutoff에서2/16 완료·해제로 종료했고47218개 부분 출력을 보존했다.
16요청 모두 첫1024 생성 토큰을 받은 시각은 cap0 940.498s→cap4 591.322s다. 공통 prefix 진단이며 완주 TPS 개선율은 아니다.
기준도 선두 계산1/16로 품질 미승인이다. 전체 비교와 종료 오류를 증거 문서에 보존했다.
수치의 분모·동일 호스트 대조·내용 실패·봉인 범위는
[긴 실기 기록](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#long-output-20260911)을 따른다.

**긴 실기 이후 다음 첫 구현:** V1.1-0의 live/retired receipt byte, enqueue/dequeue, 미발행 사유와
head 단조시계 issue→settle를 실제 소비 경로에 결속한다. worker 내부 ingress는 dequeue 이전 queue 대기 계측이 아니다.
그다음 V1.1-1의 B2/B3 예약·broker byte 퇴역을 duplicate/replay/결과 불명 보존과 함께 닫는다.
누적 payload와 RSS의 근접만으로 heap 원인을 확정하지 않으며 단순 receipt 삭제나 resident 상향으로 우회하지 않는다.
그 뒤 고정 창 안의 prefill quantum64/128/256, decode 서비스 예약·aging·tail coalescing deadline을 단일 축으로 평가한다.
MI의 산술 실패는 동일 prompt/native 품질 기준선으로 분산 경로 영향과 구분한다. 새 기본값 승격 및 flight 창 확대는 보류다.

**이번 결과가 정하는 다음 작업:**

1. V1.1-0 계측에 **host별 CPU 예산/실행 대기, native CPU 연산·spin·graph 준비/재사용**을 추가한다.
   cap4min4는 graph reset이 줄어도 느렸고 CPU4에서는 완주했으므로 graph miss 하나로 원인을 확정하지 않는다.
   native 프로세스마다 호스트 전체 CPU를 기본 할당하는 구성을 피하도록 배포 계획에서 host별 예산을 명시한다.
   threads4는 이번 진단값이며 CPU expert 오프로딩이 큰 Hy3 등에 전역 기본값으로 전파하지 않는다.
2. READY 이전 정지의 native bind 오류/errno·stdin liveness join을 계측하고, 점유 포트/재시작 반례에서
   bounded failure와 child 회수를 검증한다. 임시 포트 변경만으로 제품 결함을 닫지 않는다.
3. V1.1-1 B2/B3·receipt 수명/종료를 닫은 뒤 V1.1-2의 비용 기반 묶음·공정성을 진행한다.
   묶음 크기는 단계 겹침뿐 아니라 host CPU 예산과 실제 native 서비스 시간으로 평가한다.
   GPU/RPC 사용률만으로 더 많은 flight를 허용하지 않고 node 실행1/decode outstanding≤1을 유지한다.
4. Hy3 cap2와 MI의 CPU 예산을 명시한 후보를 **GPU 비간섭을 확인한 시간대**에 동일 조건 반복으로 재선별한다.
   per-process GPU 목록/메모리·CPU 사용과 외부 작업의 시작/종료를 함께 봉인한다. 유망 후보에만 실제100k prefill/
   decode 혼합·연속 웨이브·긴 정상 응답과 H5 paired8쌍/holdout4쌍을 적용한다. 새 정책 기본값0은 유지한다.

**현재 개발 순서는 이 절로 이관한다.** 아래 v0.9.0/P/U 기록의 당시 “다음”을 다시 직렬 선행 조건으로 만들지 않는다.
v0.9.0 봉인 여부를 이번 분석으로 바꾸거나 v1.1 구현 완료로 표시하지 않는다. 버전 bump/tag는 아직 하지 않는다.
감사 HEAD `11dc7a0ce`, MI250 실행 `f3658f1b`, Hy3 adapter/native `9ad366f9063`의 근거를
[통합 진단](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md)에 병합했다.
서로 다른 upstream/모델/backend/워크로드의 TPS를 합치지 않았다. 판정 계약은
[v1.1 검증 적용표](distributed-batching-verification.md#v11-gates)를 따른다.

**수정 목표:** 노드별 backend 실행 1개를 유지하면서 독립 batch를 여러 stage에 흘린다.
prefill·decode의 배치 구성과 서비스 시간을 분리해 장기 prefill의 지연 전파를 줄이고,
수용/전송/반환/receipt 메모리가 요청량에 따라 무제한 증가하지 않게 한다.

| 측정·코드 근거 | 계획에서 바꾸는 것 |
| --- | --- |
| MI250 16-stage 두 머신 각각 최대 동시 RPC 1; 16요청이 한 묶음에 소진 | decode 요청 묶음 크기를 별도 제어. max_issue_rows=64도 16 decode를 모두 담으므로 이것만으로 해결하지 않음 |
| MI250·Hy3 모두 issue gate 거절 0; MI250 유효 open 설정 미봉인 | open-batch 상한이 원인이라고 단정하지 않고, 실제 eligible/blocked/flight 원인을 먼저 계측 |
| Hy3 decode 간격 중앙 1.869s → prefill 구간 약 30s; 같은 길이 요청의 TTFT 약 2배 차이 | prefill 서비스 시간 예산·요청별 quantum·누적 공정성 도입 |
| Hy3 agent RSS와 payload 두 번 보관의 예상량이 약 20GiB로 근접; heap 귀속은 미확정 | active credit뿐 아니라 완료 receipt의 byte 보관/퇴역 계약을 별도 수정 |
| 8-stage 301.63과 16-stage 7.64는 resident256/16, 짧은 입력/100k 혼합 등 조건 상이 | 노드 수 손실률·최적값·유효 TPS 개선으로 승인하지 않고 토폴로지별 기준선을 새로 고정 |

#### 구현 순서와 종료 조건

| 단계 | 수정 단위 / 소유 | 종료 조건 |
| --- | --- | --- |
| V1.1-0 측정 결속 | OUTER/driver 및 adapter trace: 유효 환경설정, 요청별 수용·eligible·blocked 사유, head 단조시계 issue→settle와 flight 수, stage queue/native/forward 분해. MI250에 실제 OUTPUT 수신시각 수집 경로 결속 | 동일 artifact로 요청 묶음·실제 ITL·flight/byte 수명·미분류 시간을 재계산 가능. 노드 간 시계 오차 범위 없이 전역 겹침/홉 비용 확정 금지 |
| V1.1-1 예산과 종료 | adapter B2/B3 수용·반환 예약; core broker는 backend 중립 byte/receipt 수명 계약. pending prompt, KV/보조 상태, 전송 payload, 출력, 완료 receipt를 각각 제한. 정상/거부/부분 전송/취소·timeout의 정산·회수 결속 | 한계 초과는 부작용 전 거부, 중복 replay 의미 보존, 결과 불명은 보존. cap1에서도 제어 진행, 장기 반복 후 live/retired 예산 안정, 모든 stage 해제 후 UNLOAD. resident·flight 창 확대 전 필수 |
| V1.1-2 배치 구성 | adapter scheduler/drive: `decode_member_cap`에 해당하는 요청 묶음 선택과 prefill 행 quantum을 독립 정책으로 추가. ready decode 서비스 예약, 요청별 누적 서비스 deficit/aging, 단계별 관측 비용에 따른 prefill 시간 예산 | 같은 16요청으로 서로 다른 decode 묶음 발행. 느린 prefill이 있어도 decode/prefill 양쪽 starvation 없음. 불변 issued membership, decode outstanding≤1, KV prefix·atomic verify/replay 유지 |
| V1.1-3 파이프라인 창 | adapter: prefill fragment 1→2→4→8을 단계별로 검증. node 실행 credit=1, global open-batch credit=N, edge row/byte·receiver credit 별도. 필요 시 native 작업 중 제어/전달을 처리할 수 있게 worker 상태기계 분리 | 같은 sequence fragment의 stage 순서·정산 순서, out-of-order 도착/중복/취소에서 원장·KV 안전. 실제 소비 경로/변이로 확인 후에만 실험 기본값 확대. shared context 동시 호출 금지 |
| V1.1-4 실기 승격 | 고정 topology별 짧은 대조 실험 → 후보 선택 → 100k 연속 웨이브/긴 정상 응답·오프로딩·장기 반복. CUDA/Metal Hy3와 ROCm Step 결과를 각각 판정 | 아래 행렬과 H5 반복·holdout/SLO/품질 게이트 통과. 실패 원자료·cleanup 보존. 지원하지 못한 backend/model/기능은 명시적으로 미승인 |

V1.1-0이 **다음 첫 구현**이다. 첫 산출물은 봉인된 두 기준선의 실제 knob/요청 상태/수신시각/flight trace와
그 trace를 검증하는 소비 경로 시험이다. 기존 UTF-8 69~113토큰 실패 입력도 보존·재현하여 원인 수정 및
한글 장문 회귀를 결속한다. 64토큰/영문 성공을 해당 결함의 해결로 읽지 않는다.

V1.1-2는 고정한 안전 예산 안에서 시작하며, V1.1-3의 창 증가는 V1.1-1과 fragment 안전성 뒤다.
`queue.is_running()` 또는 `outstanding>0`을 일괄 제거하는 변경은 이 계획에 없다.
현재 event 경로에서 전자의 호출을 찾지 못했고, 후자는 decode 의존성이다.
step별 동기 호출의 완료를 기다리는 구조를 변경할 때는 준비한 실행 권한/원장 commit/외부 효과를 분리해 검증한다.

배치 선택의 구현 초안은 다음과 같다. 아래는 현재 구현 사실이 아니며 반례로 정책을 확정한다.

1. 요청별 KV prefix·미반환 fragment·예약 상태로 합법적 후보를 만든다. 단순 ready 행 총량과 구분한다.
2. 모든 요청을 매번 넣지 않고 phase별 요청 묶음을 선택한다. decode 묶음 상한과 요청별 prefill quantum을
   독립 적용하되, 혼합 batch가 다시 모든 독립 요청을 점유하는지도 검사한다.
3. decode의 대기 목표와 prefill의 누적 미서비스량/aging으로 몫을 배분한다. ready decode가 없으면 그 몫을
   prefill이 사용한다. 행 수뿐 아니라 최근 단계별 비용으로 다음 native 작업 시간을 예상한다.
4. node 실행·전체 flight·edge/receiver byte·KV/보조 상태·반환 예산을 모두 만족하는 만큼만 발행한다.
   다른 묶음은 다음 issue 기회에 즉시 선택 가능하게 두며 배치를 채우려고 무조건 기다리지 않는다.
5. issue membership/token range와 예약을 원자적으로 확정한다. 반환은 stage 완료·정산 권한에 맞춰 각각 처리하고,
   출력/해제/receipt 퇴역까지 별도 수명을 추적한다. 전송 credit 반환만으로 KV를 재사용하지 않는다.

prefill 시간 예산은 다음 작업의 크기를 고르는 예상 목표다. 실행 중 native kernel을 선점하는 보장은 아니다.
예상과 실제 비용 차이·prefill 최장 대기·decode ITL을 함께 기록하여 큰 chunk나 decode 우선의 기아를 검출한다.

#### 실험 행렬 — 작은 판별 실험부터

각 행에서 명시한 정책 하나만 바꾼다. 모델·cut·backend·KV/오프로딩·resident·도착열·샘플링·출력 종료조건과
논리/물리 batch 상한은 고정한다. 적재 시간과 추론 시간을 분리한다. 짧은 판별 arm은 사전 15분 추론 상한으로
설계하고 실패/미완을 보존한다. 3회 반복은 후보 선별용이며 성능 승인 반복을 대체하지 않는다.

| 탐색 축 | 초기 범위 | 확인할 반증 |
| --- | --- | --- |
| 독립 decode 묶음 | MI250 resident16에서 16/8/4/2요청; Hy3 resident8에서 8/4/2 | 작은 묶음으로 겹침이 늘어도 kernel 효율 감소로 TPS/SLO가 나빠지는가 |
| prefill quantum | 64/128/256/512행, 선택한 decode 묶음 고정 | 긴 native step·decode 대기가 줄어드는가, prefill TTFT/공정성이 악화되는가 |
| 요청별 prefill 창 | 1/2/4/8 fragment, quantum/전체 창 고정 | 다른 요청의 KV/edge 예산을 잠식하는가, 동일 sequence 순서가 안전한가 |
| 전체 open 창 | 1/2/4/8/16 중 byte/메모리 예산이 허용하는 값 | 실제 flight가 한계에 닿는가, 추가 창이 처리량 대신 큐/메모리만 늘리는가 |

사용자가 제시한 `(fragment, issue rows, open)`의 `(2,256,2~4)`, `(4,128,4~8)`,
`(8,64~128,8~16)`은 위 단일 축 판별 후의 **조합 탐색**으로 보존한다. 모두 최적값이 아니며
decode 묶음 상한은 별도다. 처음부터 전 조합을 6시간씩 실행하지 않는다.

최종 부하는 (1) 짧은 decode 다수+100k prefill 혼합, (2) 실제 긴 입력 여러 건의 연속 웨이브를 구분한다.
100k context 설정만으로 100k prefill 완료를 주장하지 않는다. KV를 device에 먼저 예약하고 남는 예산에
weight를 배치하며 CPU 오프로딩을 허용하되, 계획/실제 peak RAM·VRAM 및 부족 구성 거부를 검증한다.
Mac 통합 메모리는 RAM/VRAM을 서로 독립된 두 자원처럼 중복 합산하지 않는다.

평균 동시 RPC 4~8은 16-stage의 탐색 목표일 뿐 출시 게이트나 GPU 포화 선언이 아니다.
H5의 paired 최소 8쌍·holdout 최소 4쌍, 유효 TPS 중앙 개선≥5%·95% CI 하한>0,
TTFT p95≤1.10배·ITL p95≤1.05배와 사전 절대 SLO를 그대로 적용한다.
고정 길이/ignore_eos 부하와 정상 EOS·내용 품질 승인을 분리한다.

#### 범위 경계

이 버전의 필수 범위는 측정 결속, 예산/종료, 공정한 phase별 구성, 검증된 bounded flight, 선언 구성의 실기다.
physical capsule 조기 전송은 native codec/정산 계약을 바꾸는 별도 후보다. 위 변경 뒤에도 전체 capsule
반환 대기가 주원인으로 남는 trace가 있을 때만 범위를 재심사하고, 없으면 다음 버전으로 넘긴다.
병렬 sampler/shared context 실행, 검증하지 않은 recurrent/hybrid fragment 확대, 영속 KV/K gate 전체,
모든 모델/backend 승인을 이번 수정의 자동 완료 조건으로 추가하지 않는다. 지원 범위 밖은 기본 비활성/미승인이다.

아래는 이전 실행 이력이다. 당시 후보 파일·실패·미실행 기록은 보존하며 이번 계획의 새 순서를 덮어쓰지 않는다.

### Historical v1.1 cluster comparison preparation (2026-09-11)

This isolated experiment tree retains the Hy3 fleet base `1a848a716`, including
its explicit physical-wire-v4 compatibility checks and approved OUTPUT receipt
timing, and ports only the bounded adapter selection/observation change from
`d5256af44`. Driver fixture initializers set the optional scheduling field to
None. It does not replace the fleet native binaries or relax identity checks.
The source is identical for baseline and candidate; only decode-member caps
change (Hy3 0 to 2, MI250 main source 0 to 4). Resident, native batch limits,
prefill fragment count, model cuts, KV placement and offloading stay fixed.
Short screening uses 256-token length termination and a 15-minute inference
deadline; it is not normal-response quality or H5 performance acceptance.
Combined-tree validation: `cargo test --workspace --no-fail-fast --locked
--target-dir F:/dev/p4/target/v11-hy3-tests`, exit 0, 1385 passed / 0 failed /
7 ignored, 58 summaries. Runtime comparison remains pending.

## 0. 현재 상태 — 후보 보존 및 별도 fleet·upstream 통합 진행

### 0.-6 최신 사용자 지시 — MiMo 보류, Hy3로 실행 (2026-09-10)

사용자가 Hy3를 선택했으므로 MiMo 로더 수정/예외 승인은 현재 실행의 선행 조건이 아니다.
Hy3 Q5_K_S 191.884 GiB, 5호스트 6-stage, 세션당102400·resident8·KV 풀819200으로
짧은 정합성 실행 `hy3-100k-smoke-lowport-1789027462065`가 **2/2 EOS·완료·해제·UNLOAD**했다.
두 전력/에너지 문제 전문에서 수치·단위·온도 판단의 한계를 확인했다. error/cleanup_error는 null이다.
여섯 stage의 topology/shape 및 model/context/compute 계획=실제 할당을 확인했다.
긴 입력·8개 활성 세션·슬롯 재사용·포화·성능 승인은 아직 아니다.

컷은 M42 [0,16)/[16,32), Spark [32,59), Mac [59,62), 로컬3090 [62,78), Ubuntu [78,80).
CUDA KV q4_0/q4_0, Mac Metal f16/f16; batch512/UBATCH256. KV/compute를 device에 확보한 뒤
M42 각15개·로컬15개·Ubuntu2개 층의 routed expert를 RAM에 둔다. 통합 메모리를 중복 합산하지 않는다.
같은 컷의 r16 로컬 계획은 거부됐다. 다른 컷의 r16까지 불가능하다고 일반화하지 않는다.
첫 실제 LOAD는 Mac native53021 연결 EINVAL로 실패해 보존했다. 전용 agent를 새로 시작하고
native23021/23022로 재시도했다. ephemeral 충돌은 후보 원인이며 재접속 내구성 승인은 아니다.

긴 단일 실행 `hy3-100k-single-1789029008524`도31643 입력 → 2316 생성 → EOS·해제·UNLOAD했다.
네 기록의 산술·단위·인용·경보 판정은 맞지만, 경보 기준/안전 한계 혼용과 인과 표현 과장 등으로
**전체 응답 품질은 미승인**이다. 실제1418단어는 요청한 약1800~2500단어보다 짧다.
TTFT1162.465초, 생성 토큰 수신 간격 p50=513ms/p99=691.6ms. 프리필 UBATCH124개 중123개가256행이다.
단일 요청의 decode1행과 이를 합산해 전체 채움률을 병목으로 읽지 않는다.

다음16건은 **진단용 부하**로 봉인한다. 품질 미승인 입력을 포함한 같은 프롬프트와
최소1024 생성·EOS·정합성 기준을 유지하고, 정상 응답 승인과 raw 처리량을 분리한다.
입력 길이31643/63682/91696토큰, 합계966748토큰·4614390바이트이며 max8192와 합쳐 최대99888토큰이다.
8건을0~56초에8초 간격, 다음8건을300~356초에 제출한다. inference 제한6시간, 관측기 상한8시간이다.
prefill_fragments 기본1과 outstanding 의존을 유지하며 도착 시차로 독립 배치 진행 기회를 만든다.
OUTER의 승인 OUTPUT별 수신 시각은 전송 효과를 포함하며 GPU 완료 시각이 아니다.
RAM에 둔 웨이트도 큰 배치의 연산은 CUDA로 옮겨질 수 있어 CPU 계산으로 단정하지 않는다.
모델6개 전체 파일 해시와 장비별 실행 중 라이브러리 해시를 확인했다. PCIe 표본도 다음 실행에 보존한다.
정확한 head 원장 flight 시간 분포와 반복 포화 비교는 별도 미완이다.
[Hy3 증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-fleet-latest-integration.md#hy3-100k-2026-09-10)를 따른다.

16건 실행 `hy3-100k-wave-diagnostic-1789033111493`은 6-stage 적재·계획=실제·라이브러리 해시를 확인했지만
프리필 중 정체돼 **진단 중단**했다. 전달16/완료0/해제0, head 관측 UBATCH82개는 모두256행이다.
M42 호출 완료41/41이 약508초간 늘지 않는 동안 로컬3090만 높은 활동/PCIe 전송과112 MiB 여유를 보였다.
메모리 압박은 우선 가설이며 확정 원인이 아니다. 검증 native를 의도적으로 종료했으므로 뒤따른10054는
유발한 오류다. 부분 artifact·최초 오류·busy UNLOAD·누락 증거를 보존했다. 완료된 RPC 구간만의 사용률은
정체 구간을 제외하므로 전체 추론 사용률로 인용하지 않는다.

**최종 판정 (2026-09-11):** `hy3-100k-wave-headroom-1789035735558`을 사용자가 선택한 원래6시간
제한까지 실행했다. 16건 전달, **4건 EOS·완료·해제 /12건 미완료**, 두 번째 웨이브8건은 OUTPUT0이다.
최초 오류는 deadline이며 busy UNLOAD는 별도 cleanup_error다. 승인 OUTPUT16438개와 부분 산출물을 보존했다.
누락된 요청/실행 행 수를 복원하지 않으며 raw/품질 승인 TPS는 미산출이다. 완료4건 전문의 계산16항목은
맞지만 근거 없는 인과 배제 등과 요청 길이 미달(1110~1201단어) 때문에 전체 정상 응답은0/4다.

실제 출력이 있었던8건의 TTFT p50=94.85분/p90=166.97분이며 나머지8건은 미관측이다.
완료4건의 요청 도착→완료 p50=309.34분이다. head가 수집한 물리 prefill-only2137개는 모두256행,
mixed328개 중325개는256행, decode-only3783개는 평균4.156행/최대7행이다.
**프리필 폭은 찼지만 연속 요청의 완료·지연·decode 활용 목표는 충족하지 못했다.** head 전체 원장 flight
시간 분포와 대기 귀속은 미완이므로 이 숫자만으로 스케줄러/전송/CPU 중 원인을 확정하지 않는다.

로컬 layer77 전문가를 RAM으로 옮긴 v3의 여섯 stage는 계획=실제이며 로컬 device 요구량은
20203350016바이트(KV15099494400)다. 전체 관측창 최소 여유2548 MiB를 유지하며 추론은 진행했다.
다만 이전 실행은 재사용 agent, 이번은 fresh agent라 **엄밀한 한 변수 비교가 아니며 이동만의 효과는 미확정**이다.
Spark agent RSS는 관측 시작 후7.54→20.55 GiB, 시스템 가용 RAM 최저2.73 GiB였다. full Event를
건수 제한으로 보관하는 broker 원장은 확인된 메모리 증가 후보지만 힙 귀속 증명은 아니다.

실행 소스/입력/바이너리를 끝까지 유지했다. 원자료를 수집한 뒤 해당 검증 native/agent만 정리했고
Mac 전용 GUI agent는 새 PID1560/52004 LISTEN으로 복구했다. 프로세스 종료는 UNLOAD 성공이 아니다.
로컬4080의 기존 작업은 유지했다. 이 기록은 실험 마감이며 제품 릴리즈/최종 목표 달성 선언이 아니다.

**다음 버전 첫 행동:** 장시간 부하를 반복하기 전에 (1) broker/수용/반환의 바이트 예산과 안전한 receipt
retirement, deadline 뒤 cancel→drain→release→UNLOAD 경로를 실제 소비 시험으로 닫는다.
(2) 같은 배포에서 짧은 다국어 정상 응답을 검증한다. 이번 영어 출력에 UTF-8 오류가 없다는 사실은
별도 MI250의69~113토큰 오류를 고친 증거가 아니다. (3) 고정된 컷/초기 상태에서 짧은 단일→동시2/4/8을
단계별로 측정하고 prefill/decode별 대기·CPU/전송·head flight를 귀속한 뒤 같은100k arm을 재실행한다.
resident 상향이나 Metal KV/컷 변경은 실제 메모리/정상 응답 비교를 통과한 뒤 별도 arm으로 수행한다.

### 0.-5 이전 사용자 지시 — 최대 성공 모델·세션당 100k (2026-09-10)

현재 우선순위는 과거 실제 LOAD 성공 모델 중 가장 큰 **MiMo-V2.5 UD-Q5_K_S, 201.446 GiB**를
기존 M42·Spark·Mac .21·이 PC·Ubuntu에 적재하고 긴 입력/생성 웨이브를 검증하는 것이다.
TUF/Mac .20 복구를 이 실행의 새 직렬 선행 조건으로 만들지 않는다. 아래 맥 포함 122B 결과는 과거의 별도 성공이다.

**현재 판정: 적재 전 RED.** 봉인 제품 소스 `9ad366f90`의 CUDA·Metal 바이너리로 5호스트 6단계
no-alloc 계획을 실행했으며 전부 `sliding_window_pattern ... expected 48, got 51`로 exit7했다.
MiMo의 과거 성공은 이전 pin의 LOAD 증거이며 현재 pin의 지원 증명이 아니다. 새 메모리 계획·실제 LOAD·추론·TPS는 없다.
원인과 원자료는 [MiMo 사전 판정](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-fleet-latest-integration.md#mimo-100k-preflight-2026-09-10)을 따른다.

1. **로더 회귀부터 복구한다.** 새 공통 로더가 NextN 수를 먼저 읽는데 MiMo SWA 배열 독자가
   여전히 본체 층 수를 기대한다. 수정 후보는 그 독자의 `n_layer()`를 `n_layer_all`로 바꾸는 한 줄이다.
   현재 후보는 미빌드·미적용이다. 공식 upstream 변경/PR 근거를 확보하거나 사용자가 이 변경의 로컬 검증 예외를
   명시 승인해야 현행 모델 격리 검사와 충돌 없이 채택할 수 있다. 가짜 PR provenance, GGUF 배열 절단,
   공통 loader의 배열 길이 검사 완화로 우회하지 않는다. 51/3 정상, MTP 없는 정상, 잘못된 배열 거부,
   실제 GGUF 소비와 독립 재컴파일 변이, CUDA/Metal 및 기존 122B 회귀가 완료 조건이다.
2. **세션당 102,400토큰, resident 8/16/32를 계획한다.** 입력+출력 한도이며 전체 KV 풀은 각각
   819,200/1,638,400/3,276,800토큰이다. `context_size=102400`, `total_context_size=102400*R`,
   native `--ctx-size=102400*R`, `--n-seq-max=R`를 함께 검증한다. 세션 수만 올리고 풀을 100k로 두지 않는다.
   초기 컷 [0,8)/[8,16)/[16,36)/[36,40)/[40,46)/[46,48)은 **미승인 후보**다.
   KV와 compute 및 여유분을 먼저 확보하고 남은 device 공간에 웨이트를 넣으며 초과 routed experts는 PC RAM에 둔다.
   KV host fallback을 허용한 것으로 읽지 않는다. 계획/실제 context buffer 위치를 확인하고 M42 두 stage의 host 합계를 검사한다.
   Spark·Mac의 통합 메모리는 중복 합산하지 않는다. 실제 계획 전에는 이 컷의 적합성·동시 세션 상한을 확정하지 않는다.
3. **의미 있는 긴 입력을 사용한다.** 32개 문서를 실제 tokenizer로 31,645~92,165토큰 검산했고,
   모델 Jinja template와 32/32 동일하다. 앞/중간/끝 기록을 인용하는 전력·에너지 계산과 비교 보고서이며 독립 정답표가 있다.
   최대 생성은 8,192토큰(최장 입력+출력=100,357), 완결 EOS와 최소 1,024 생성토큰을 별도 수락한다.
   `length`나 짧은 답변을 성공으로 바꾸지 않는다. 계산 4건·단위·출처·비약·결론을 전문 검토하고 품질 통과 TPS를 분리한다.
4. **측정 순서는 정상 응답 → 긴 단일 입력 → 동시성 → 지속 웨이브다.** 짧은 정상 게이트는 100k 부하 승인을 대신하지 않는다.
   resident 8/16/32에서 배치 512/UBATCH512를 고정한 비교부터 한다. 수용 가능한 resident를 고정한 뒤 head agent의
   `P4_STAGED_MAX_OPEN_BATCHES=1/2/4/8`을 비교한다. 이 값은 native NodeConfig environment가 아니라 agent 환경이다.
   `outstanding>0` 의존은 유지한다. 32개 입력의 길이 혼합으로 prefill/decode 동시 준비 기회를 만들고,
   최소 2회 슬롯 재사용 웨이브와 3회 반복을 확보한다. 뒤 웨이브의 입력 선택·도착 간격·재사용 여부는 선행 계측 후 봉인한다.
   매 실행에서 제출 수/바이트를 제한하며 전체 서비스 B2/B3 boundedness 승인을 주장하지 않는다.
5. **포화는 실제 진행으로 판단한다.** prefill/decode/mixed별 물리 폭·512 충족 비율, ready/eligible 차이,
   head issue→retirement 기준 비행 수의 시간 분포, stage 큐 대기·RPC·전송, 장비별 GPU/RAM/VRAM·swap,
   요청별 TTFT·실제 연속 토큰 간격·기아·완료/해제/UNLOAD를 보존한다. 현재 `StageSpan`만으로 정확한 head 원장
   비행 수를 확정할 수 없으므로 해당 계측은 열린 선행 작업이다. 호스트 시계 오차도 기록한다.
   RPC 겹침과 GPU kernel 활성 비율을 계산 포화로 치환하지 않는다. 반복 분산 안에서 개선이 멈추는 구간을
   시험 범위의 plateau로 보고하며 전역 최적이라고 하지 않는다.

이 단계는 사용자 요구의 실행 준비 및 실제 RED 보존까지다. 제품 수정·100k 적재·연속 부하 완료가 아니다.

### 0.-4 이전 사용자 지시 — 맥 포함 재시험 (2026-09-10 16:02 KST 실행 종료)

**M42·Spark·Mac .21·이 PC·Ubuntu, 다섯 물리 호스트의 CUDA·Metal 혼합 122B 실행을 통과했다.**
새 CREATE/DELETE 왕복이 같은 Mac agent PID78595/바이너리에서 성공했다. 이 턴에서 Mac 권한·서명·실행 파일·방화벽을 변경하지 않았으며, 외부 복구 원인은 미확정이다.
소형 `gemma-five-hosts-cuda-metal-1789022821079`가 8/8 완료·EOS·해제·UNLOAD한 뒤, 에이전트 재시작 없이 122B를 실행했다.

122B `122b-five-hosts-cuda-metal-1789022821180`: **32/32 완료·EOS·해제·UNLOAD**, error/cleanup_error/evidence_missing null.
컷은 M42 GPU0 [0,8), GPU1 [8,16), Spark [16,29), Mac Metal [29,37), 이 PC 3090 [37,45), Ubuntu [45,48)다.
resident4, 4건씩 8회 웨이브, max512, 명시적 physical-wire-v4 후보이며 Mac KV는 f16/f16, CUDA는 q8/f16이다.
여섯 단계의 topology/shape와 host/device model/context/compute 계획=실제 할당을 확인했다. 실행 중 로드된 후보 파일도 단계별 해시와 일치했다.

추론창 283.612초, decode 3,339행 / **11.773 row/s**, TTFT p50 115.987초·p90 218.912초다. LOAD/UNLOAD 제외이며 정상 응답으로 승인한 유효 TPS는 아니다.
32건 전문 검토에서 발열 설명8건의 기계적 마찰 비유를 남겼다. 전체 정상 응답·지속 부하·성능 개선 승인은 하지 않는다.
Mac 전역 AGX Device Utilization 평균62.38%는 자체 RPC 창262표본의 드라이버 카운터다. NVIDIA kernel-active 표본과 의미가 달라 합산하거나 SM 점유율로 읽지 않는다.
이번에 통과한 것은 이 후보·모델·컷의 혼합 실행이다. TUF·Mac .20은 미참여이며 전체 장비와 H0~H7의 완료가 아니다.

네 CUDA 검증 agent와 다섯 monitor를 정리했고 Mac native가 사라진 것을 확인했다. 다른 Codex가 관리하는 Mac agent는 유지했다.
제품 소스 `9ad366f90`과 봉인 바이너리는 그대로다. 주 checkout의 별도 앱/설정 변경은 건드리지 않았고 검증 checkout에서만 기록한다.
원자료108개는 `F:/dev/p4-releases/mac-included-20260910-1602.zip`에 보존했다. 해시·응답·측정 범위는
[혼합 실행 증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-fleet-latest-integration.md#mac-included-five-host-retry-2026-09-10)가 소유한다.

다음 첫 행동은 TUF 인증·Mac .20 NAS 복구 후 미참여 장비를 포함한 컷/메모리 판정이다. 별도로 정상 응답 기준선·B2/B3·대기 원인·지속 부하를 닫아야 한다.
이제 Mac .21 통신 복구를 선행 차단 조건으로 반복하지 않는다. 아래는 앞선 실행 당시 기록이다.

### 0.-3 이전 재시도 — 15분 뒤 전체 장비 재시험 (2026-09-10 14:03–14:40 KST)

예정된 재시도를 수행했다. **전체 장비 및 CUDA/Metal 혼합 실행은 여전히 BLOCKED**다.
Mac .21은 새 CREATE를 받았지만 14:31:28의 응답 송신이 NECP/error65로 거부됐고,
TUF .17은 SSH 인증 거부, Mac .20은 SMB mount·P4 agent 없음이다. 다른 Codex가 관리하는 Mac agent는 보존했다.

M42의 42mob 대화형 로그인·S: 접근은 복구됐다. 별도 경로에 봉인 Windows runtime을 배포하고 13개 파일을 해시 검증했다.
전용 TCP 52004 규칙은 해당 실행 파일과 대상 IP 6개로 한정했다. 이 PC의 52004는 OS 예약 포트여서 51054로 실제 P4 왕복을 검증했다.
기존 앱·v0.9.0 파일·차단 규칙은 보존했다. 프로그램 이름 변경이나 권한 우회로 Mac 거부를 피하지 않았다.

연결 가능한 **네 CUDA 물리 호스트** M42·Spark·이 PC·Ubuntu에서 122B UD-Q5_K_S를 실제 실행했다.
다섯 stage 컷은 [0,8)/[8,16)/[16,37)/[37,45)/[45,48), resident 4, 4건씩 8회 웨이브다.
실행 `122b-four-cuda-hosts-1789017057291`: **32/32 완료·EOS·해제·UNLOAD**, error/cleanup_error/evidence_missing null.
추론창 220.856초, decode 3,362행/15.223 row/s, TTFT p50 83.673초·p90 167.304초다. LOAD/UNLOAD는 이 시간에서 제외한다.
응답 32건 전문을 읽었으며 발열 8건의 기계적 마찰 설명 때문에 전체 정상 응답·유효 TPS·서비스 승인은 보류한다.
각 host RPC 창 GPU 표본 평균은 M42 두 장 20.2/20.6%, Spark 21.8%, 이 PC 3090 22.9%, Ubuntu 7.0%다. SM 점유율·포화·개선 증거가 아니다.

다섯 stage의 topology/shape 및 host/device model/context/compute 계획=실제 할당을 대조했다.
선행 소형 arm은 첫 전송 실패/8건 미완료/UNLOAD busy를 보존하고, 전체 검증 agent를 새로 시작한 다음 8/8 완료·해제했다.
재시작 뒤 정상 통과는 peer 재접속 내구성 승인이 아니다. 이번에 제품 소스를 바꾸지 않았고 과거 단위 시험을 재실행한 것으로 세지 않는다.

원자료 154개와 해시는 `F:/dev/p4-releases/all-hosts-retry-20260910-1436.zip`에 보존하고 ZIP 내부 파일을 재검증했다.
이는 로컬 인도이며 다른 머신에서의 장기 재열람 게이트는 미충족이다. 상세·전체 실패·해시는
[증거 문서](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-fleet-latest-integration.md#scheduled-all-computer-retry-2026-09-10)를 따른다.
검증용 네 agent·모델 프로세스·네 monitor는 정리했고 예약 재시험은 중지했다. 제품 소스는 `9ad366f90` 그대로다.

다음 첫 행동은 Mac .21의 **실제 p4-agent 왕복 응답** 복구, TUF 계정/키와 Mac .20 NAS 복구다.
그 뒤 원래 요청한 M42·Spark·Mac 혼합 소형 게이트 → 122B → 모든 장비 웨이브를 판정한다.
전송 실패의 즉시 귀속/보존·재접속과 정상 응답 기준선은 별도 미완이며, 단순 재시작·resident 상향으로 닫지 않는다.

### 0.-2 사용자 추가 지시 — 전체 장비와 최신 upstream (2026-09-10)

v0.9.0 후보와 기존 게이트는 보존한다. 사용자가 Spark, TUF, Mac mini 두 대, Ubuntu 노트북,
이 PC 등을 NAS와 연결하고, 개별 모델 실행 → 모든 컴퓨터를 잇는 대형 모델 실행 → 최신 llama.cpp
업데이트 성공을 지시했다. 이 지시가 과거의 한 호스트 자원 제한과 다음 버전 대기 순서보다 우선한다.
개발 checkout은 `F:/dev/p4-fleet-20260910`, branch `codex/fleet-latest-20260910`이며 릴리즈 후보는 수정하지 않는다.

1. NAS의 실제 모델 읽기와 장비별 도구·메모리·사용 중인 서비스·포트를 확인한다. TUF SSH 인증,
   Mac .20 NAS 인증, M42 interactive logon은 사용자 입력을 기다리며 나머지 작업은 계속한다.
2. 최신 관측 pin `434ddbbc0`의 호환 패치를 재생하고 CUDA·Metal·CPU 빌드 및 실제 소비 경로 회귀를 검증한다.
   기존 split 입력 패치의 upstream 대체를 경계 입력으로 검증한다. 재생만으로 채택하지 않는다.
3. 각 장비에서 NAS의 적합한 모델을 실제로 실행하고 완결 응답·배치·해제·사용 메모리를 남긴다.
   Spark/Mac의 통합 메모리는 host와 GPU로 중복 합산하지 않는다.
4. 사용 중인 19001 서비스를 보존하고 가능한 P4 포트의 장비 간 연결을 검증한다. 합법적 컷과
   stage별 실제 계획으로 대형 모델을 모든 대상 컴퓨터에 분산한다. 작은 모델이나 일부 장비 성공으로 대체하지 않는다.
5. 정상 프롬프트·완결 응답·연속 웨이브·실제 placement·실행 ID와 바이너리 해시로 판정한다.
   B2/B3 예산 부재 등 기존 제약을 기록하며 resident 상향 서비스 승인은 별도 게이트로 유지한다.

현재 사실과 열린 게이트는 [fleet/upstream 증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-fleet-latest-integration.md)를 따른다.

최신 native 빌드·CTest 16/16은 Windows, Spark, Ubuntu, 두 Mac에서 통과했다. 최종 Rust는 1,378/0/7(58 summaries), Node는 159/0이다.
저장 계획의 CLI 호환, CPU_REPACK host 회계, 공유 no-alloc compute 중복 집계는 실제 소비·독립 변이로 검증했다.
Windows·Spark·Ubuntu·Mac .21은 NAS 모델의 개별 실행을 통과했다. Mac .20의 로컬 복사본 진단은 NAS 승인이 아니다.

122B는 Spark·Ubuntu **두 물리 호스트에서 8회 × 4건 = 32/32 완료·해제·EOS·UNLOAD**를 통과했다.
소스 `9ad366f90`, resident 4, 추론창 115.422초, TTFT p50 47.405초·p90 88.278초다.
전선 발열 응답의 부정확한 비유와 대기를 남기며 전체 정상 응답·서비스·성능 개선 승인은 하지 않는다.
각 호스트 stage RPC 창의 GPU kernel-active 표본 평균은 Spark 83.9%, Ubuntu 13.5%다. SM 점유율이나 포화가 아니다.

명시적 `physical-wire-v4` 후보는 upstream·patch·native codec/표현을 대조하고 stage별 identity를 보존한다.
기본 exact-build 거부는 유지하며, 소비 시험·독립 변이 4종과 CUDA 두 호스트/Metal 한 호스트 각각의 8/8 실행을 통과했다.
당시 **CUDA·Metal 혼합 실기는 BLOCKED**였다. Mac .21 커널이 agent의 외부 TCP를 NECP/error 65로 거부했다.
Python/nc의 포트 연결 성공은 agent의 통신 승인이 아니며, 당시 실행은 CREATE 응답 전 중단돼 추론을 제출하지 않았다. 이후 실제 응답 복구와 혼합 실행 통과는 위 §0.-4가 갱신한다.

현재 재개 조건은 위 §0.-4를 따른다. Mac .21 혼합 실행은 통과했고 TUF SSH와 Mac .20 NAS는 남아 있다. M42 로그인/NAS와 이 PC의 후보 통신 경로도 복구됐다.
미참여 장비의 접근 복구 뒤 기존 거부 기준을 유지한 모든 대상 장비의 합법적 컷·소형 게이트·122B 웨이브를 판정한다.
접근이 복구되기 전에는 추가 부분 호스트 실행을 전체 장비 승인으로 바꾸지 않는다.
현재 후보의 소스·플랫폼별 runtime·성공/실패/변이/메모리 원자료 329파일을
`F:/dev/p4-releases/fleet-20260910-wire-candidate`에 보존했다. 증거 문서의 candidate manifest가 해시를 소유한다.
실험 소유 agent/stage·monitor는 정지했고 기존 앱·v0.9.0 후보는 보존했다. 정식 tag/push는 하지 않았다.

### 0.-1 이번 버전 마감 상태 (2026-09-10)

코드 수정과 로컬 검증·패키징은 마감했다. **정식 릴리즈 승인과 다음 버전 개발을 구분한다.**
Windows Update의 계획 재시작으로 최신 r256 실기가 중단됐고, 재시작 뒤 42mob 로그인 세션이 없다.
로그인 복구 후 남은 두 게이트만 판정하면 된다. 하위 09-07 중단·WIP 기록은 당시 이력이지 현재 HEAD의 상태가 아니다.

| 항목 | 현재 결과 |
| --- | --- |
| 수정 commit | `3302591fc`: PLAN/ACTUAL 분리, Ninja Release/Debug 전달, 실제 서버 옆 CUDA runtime 복사 |
| 로컬 검증 | workspace 1,374/0/7, Node 146/0, CTest 15/15, docs-lint 91 clean, compat 26 valid, private headers 81 clean |
| 새 0.9.0 agent/drive 실기 | smoke·2B pressure·35B r96 PASS, 완료/해제/UNLOAD |
| 남은 게이트 | r256 OS 재시작 중단(실패 보존), must_refuse 미실행 — **BLOCKED** |
| 실제 후보 보관 | runtime/source/evidence ZIP, 10개 실행 원자료·실패/성공·변이 로그, SHA256SUMS/RELEASE-STATUS.json |
| tag / 원격 | 기존 미공개 tag는 archive ref에 보존, 정식 재봉인/push 보류. agent/stage 정지, 기존 배포 백업 유지 |
| 미달성 | H0~H7·다중 호스트·서비스 승인·TPS 개선·resident 상향 승인 |

현재 남은 첫 행동은 **42mob 로그인과 S: 접근 복구 → 동일 binary r256·must_refuse 판정 → 최종 tag**다.
소스·실행 ID·해시·후보 위치는 [릴리즈 노트](release/v0.9.0.md)와
[게이트 증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-release-gate-v0.9.0.md)를 따른다. 실패 입력·상한·judge를 완화하지 않는다.

그 후 **다음 버전**은 P-3d 2번의 남은 no-alloc 모델/backend 행렬부터다.
해당 2B·35B r96/r256의 각 stage host/device 계획=실제 대조는 끝났고 r160·다른 조합은 남았다.
그다음 B2/B3 수용·반환 예산 → 정상 응답 기준선·원인 trace → H5이며, resident 상향 서비스 평가는 B2/B3 뒤다.

### 0.0 2026-09-07 저녁 — 사용자 재개 지시 후 실제 확인 결과 (검증됨)

사용자가 "실제 확인 후 계속/변경 판단, 실측으로 TPS·배치 포화·GPU 사용률 보고"를 지시해 위 §0.1~0.6의
미검증 항목을 실행했다. 원자료·해시·명령은
[2026-09-07 증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-07-head-verification-and-3090x2-ladder.md)가 소유한다.

| 항목 | 결과 | 지위 |
| --- | --- | --- |
| HEAD `2ed9b71d4` release 빌드 | 성공 | 검증됨 |
| HEAD `cargo test --workspace` | 2026-09-07 시점 **컴파일 실패**(`issue_witness_tests.rs:409`, `RequestState`는 `Deref`만 구현), 시험 0개 실행. **2026-09-09 `input_mut_for_test()`로 복구: 1,361 passed / 0 failed / 7 ignored** | 해결됨 |
| 그 크레이트 제외 워크스페이스 | 849/0/0 | 검증됨 |
| 독립 워크트리 + 시험 1줄 수정 후 staged adapter | 512/0/7, **cap1 actor ring 진행 시험 통과**, cap8 통과 | 검증됨(HEAD 자체 아님) |
| cap1 후보 제거 변이(`forward_independent_front` 무력화) | cap1 실패, cap8 통과 | 검증됨(변이 1종) |
| 원격 3090×2, HEAD Release 바이너리, 09-04와 같은 launcher | **품질 판정 전** 35B VRAM-only 117.07 gen TPS(= 생성 token 38,148 / 325.868 s). 같은 실행의 decode 행 속도는 116.87 row/s(= 38,084 / 325.868)로 둘은 다른 양이다. 09-04 기준선 116.9~118.5와 동일, 2B 2-stage 189.75, 31B dense 2-stage 71.59. 35B는 거부 4건을 빼면 109.70 | 검증됨(비회귀 관측, paired A/B 아님) |
| GPU 사용률·배치 포화 | 적재·정리를 뺀 stage 실행창 평균 GPU0/1: 2B 32.9/38.1%, 35B 29.3/30.7%, 31B 42.5/34.4%, 35B 오프로딩 21.8/21.6%(전체 캡처는 15~29%). 실행창 0% 표본 1.0~29.1%. 35B decode 평균 15.80행·prefill 374.37행 | 검증됨(미개선, 유휴 원인 미분해) |
| `pressure` 512요청 | 2026-09-07 두 호스트 모두 UNLOAD `unload is busy; active_owners=224/256` 거부, 최초 오류가 덮여 판정 불가였다. **2026-09-09 판정 완료:** 최초 오류는 해제·정산 경로의 `stage control batch total receipt budget is exhausted`이고, UNLOAD 거부는 그 결과다. 전체 stage의 해제 완료가 용량 한계에 막혔다. 그 예산은 P-2 5번에서 명령별 상한과 연결했고, 같은 시나리오가 3090×2에서 두 번 모두 512/512/512로 통과했다 | 판정됨(§0.7 P-2 3번), 수정·재판정 완료(5번) |
| RAM 오프로딩 arm(원격, expert→CPU `--no-mmap`) | 35B MoE 43.19 gen TPS 수락·judge 64/64(ChatML), 품질 판정 전 117.07의 0.37배. Qwen3.5-122B-A10B는 두 stage 로드(host 38.3/39.0 GiB)까지 됐으나 tail이 `stage_memory_plan.cpp:358` host compute 계획≠실제로 exit 5 → **BLOCKED**, TPS 미측정. S: mmap 오프로딩은 페이지 폴트로 정지(30분에 stage 실행 1회) | 검증됨/BLOCKED |
| 하네스·drive 결함(2026-09-08 검토) | 최초 추론 오류가 UNLOAD 실패에 덮임, `stopChild`가 신호 종료를 정지 실패로 오판(재현 13 ms), 실패 run의 stage가 원격 agent에 잔존 | 검증됨(결함) |
| `judge.mjs` 의미 판정 | 길이·한글 비율·용어·반복·stop 휴리스틱. 31B는 32/32가 thought 표식과 `length` stop, 3건은 코드 펜스가 잘린 채 통과 | 검증됨(의미 승인 아님) |

**판단: 계획을 바꾸지 않고 B1/B2를 계속하되 첫 행동을 바꾼다.** 후보는 cap1을 실제로 고쳤고 성능을 깨지
않았다. 그러나 (1) HEAD가 시험 컴파일이 안 되고, (2) `pressure`가 UNLOAD busy로 끝나는데 원인을 가릴 수
없으며, (3) 처리량과 GPU 활용은 이번 변경 전후가 같다. 재개 순서는 §0.5의 1번 앞에 다음을 둔다.

1. ~~`issue_witness_tests.rs`의 대입을 `input_mut_for_test()`로 고쳐 HEAD `--workspace` 전체 집계를 회복한다.~~
   **2026-09-09 완료.** 1,361 passed / 0 failed / 7 ignored, staged adapter lib 512 passed.
   `DerefMut`은 여전히 없고 제품 경로는 바뀌지 않았다.
2. 판정을 가리는 하네스·drive 결함을 먼저 고친다. `run/mod.rs`가 UNLOAD 전에 `InferenceResult::error`와
   부분 결과를 보존하게 하고, `run.mjs::stopChild`가 신호 종료를 정지로 인정하게 하며, 실패 경로에서
   원격 stage를 정리한다. 각각 실패하는 시험을 먼저 남긴다.
3. 그 뒤 `pressure`를 재실행해 UNLOAD busy가 해제 누수인지 추론 중단 뒤의 정상 거부인지 판정한다.
   `stage_owners`/`stage_frontiers`가 정산 뒤 해제되지 않는지, `2e9451a5c`의 검사가 과잉인지는
   그 재판정 결과로 정한다. 판정 전에는 회귀로도 정상으로도 부르지 않는다.
4. 하네스 `prefill_mix_35b_2stage`가 ChatML 모델에 gemma turn을 보내는 문제를 H1 수용 전에 고친다.
   `judge.mjs` 휴리스틱만으로 H1을 통과시키지 않는다. 표식·절단 응답 검사를 함께 정의한다.
5. RAM 오프로딩 확장(B6/B7의 자원 단계 3)을 막던 tail stage의 host compute buffer 계획≠실제 판정
   (`stage_memory_plan.cpp:358`)은 2026-09-08 `0025-noalloc-reserve-size-max.patch`로 해결됐다
   ([증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-08-noalloc-plan-underestimate.md)).
   **적재 거부가 사라졌을 뿐 122B급의 정상 응답·처리량은 여전히 미측정이다.** `p4-event-drive`의 2노드 최소 조건 때문에
   1-stage 오프로딩 기준선은 지금 표현할 수 없다.
6. 그 뒤 §0.5 1~6을 그대로 진행한다. GPU 유휴와 ubatch 채움은 B4/H5의 대상이지 이번 후보의 성과가 아니다.
   유휴를 주장하려면 같은 분석창에서 **준비된 합법적 행과 장치 유휴를 함께 담은 trace**가 필요하다.
   전체 캡처 사용률·phase 혼합 ubatch 채움은 그 근거가 아니다.

### 0.1 결론과 확실한 증거 경계

**정합성 기반과 실제 결함 재현은 전진했다. 그러나 완성된 분산 인플라이트 배치, 최근 수정의
교착 해소, 유효 TPS/GPU 활용 개선은 아직 증명하지 못했다.** 시험 수 증가와 WIP 커밋 수는
사용자 목표 달성량이 아니다. 마지막 검증 이후에는 미검증 작업량만 늘어난 구간이 있었다.

| 분류 | 확실히 말할 수 있는 것 | 말할 수 없는 것 |
| --- | --- | --- |
| 기준 `a9e1967fc` | 요청 정산 함수 공유, 요청별 fairness 반례, 실제 tail codec 검사. 당시 Rust844/0/7 | 전체 반환 원자성·실제 분산 scheduler 완성 |
| 후속 검증 소스09/12 | 아래 정산·원장·worker 소비 회귀 및 제한된 ACK 진행. 각각 Rust1253/0/7 | 순환망 교착 자유, 모든 자원 예산, 최신 HEAD 통과 |
| 마지막 실행13 | Rust1254/1/7, cap1 교착 RED와 cap8 동일 입력 양성 대조 | 현재 수정 후보가 RED를 고쳤다는 주장 |
| RED 뒤 7개 WIP + 현재 후보 | 코드·시험이 작성돼 Git/작업 트리에 존재 | 컴파일·회귀·변이 통과, 처리량/메모리 개선 |
| 최종 제품 성과 | 과거 작은 모델/35B·한 호스트 실기 자료는 존재 | 이번 변경의 실기 성과나 다중 컴퓨터 초대형 모델 최종 승인 |

마지막 실행은 `cargo test --workspace --no-fail-fast --locked`,
2026-09-07 04:29:42~04:32:06 UTC, exit101, 57개 summary다. 유일한 실패는
`event_actor_ring_saturated_normal_ingress_must_progress_without_external_dequeue`의
최종 `normal_progress` 단언이다. 이 결과는 `f13e2560b`에 actor 반례를 추가해 봉인한
400개 입력에 귀속한다(이후 반례 보존 커밋 `393a6c23e`). 최신 HEAD의 결과가 아니다.

원문 `target/capacity-slice-20260907-13/workspace-result.json` 및 `workspace.log`,
source SHA256 `4a353e02335162a53371df334f8bd55b53742745392178cfb99ec1dc8ddb49eb`,
log SHA256 `a2889eef868a1b68ab732df88c7afd46ff4aa38416b0ad31848886eb318f9ce7`.
상세 실행/EXE/변이는 [정산 증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)가 소유한다.
`target/` 원자료는 이 머신의 보존물이지 다른 머신에서 재현 가능한 장기 증거가 아니다.

### 0.2 실제로 검증된 진전

아래는 마지막 검증까지의 해당 반례에 한정한다. 현재 WIP 전체로 소급하지 않는다.

| 진전 | 실제 소비 경로·대표 시험/증거 | 남은 한계 |
| --- | --- | --- |
| 반환 전체 사전 검증과 발행 대조 | `worker/release.rs`, 정산/발행 원장; T10 원장 보존, T11 한 멤버 오류의 전체 거부, T12 이전 execution의 다음 fragment 소비 금지 시험이 실행13에서 통과 | 모든 replay/native side effect·crash 수렴까지 완료한 것은 아님 |
| 발행 상태와 stage 실행 권위 | 공유 issue/settle·FlightLedger·Prepared/AwaitingNative/Uncertain, incarnation·BindLoad·physical receipt·stage KV frontier 관련 소비 회귀 | 전 구간 credit/remote acceptance·영속 exactly-once 증명 아님 |
| 공유 정산의 실제 모델 경로 보호 | `Simulation::advance`를 통과하는 malformed arrival 시험도 실행13 통과 | 가짜 엔진 모델과 실제 transport/메모리 비용은 다름 |
| 실제 worker 소비 시험 확대 | 일반 2/4/8-stage, speculative 2/4-stage, head 승인 후 OUTPUT·제어/해제 identity·busy UNLOAD 관련 회귀가 기본 실행에 포함 | fake native/post-LOAD 범위; llama/CUDA/원격 정상 응답 실증 아님 |
| 출력·해제의 실제 생산/소비 결속 | head OUTPUT, 요청별 RELEASE 통지·발행 witness·관측·문자열 byte codec의 consumer 대조 | peer/source 인증·자연어 품질·실제 GPU placement 증명 아님 |
| completion Full 중 제한된 ACK 진행 | `Worker::run`/`ack_service.rs`; genuine ACK 시험 및 Full non-ACK FIFO 복구, pending receipt ID 의무의 사전 거부/사후 의도 보존. 독립 변이5종 검출 기록 | non-ACK 앞단과 EventNode/broker까지 함께 찬 순환은 해결하지 못함 |
| 교착을 단위 함수가 아닌 실제 순환에서 고정 | `actor_ring.rs`가 실제 EventBroker→EventNode→adapter→Worker를 사용. cap1 정지, cap8 같은 입력 완료. 외부에서 C1~C6을 꺼낸 뒤 정확한 복구 | 교착 원인의 도달성 증거이지 정상 진행 성공이 아님 |
| 인수인계·계층 계약·Git 보존 | B/T/I/H/K 책임, event 실행 경로, source/EXE 결속과 오류 집계 규칙 정리; 누적 변경의 체크포인트 생성 | 문서량·커밋량은 실행 증거가 아니며 원자료 장기 보존 미완 |

### 0.3 작성됐지만 검증되지 않은 것

`393a6c23e` 뒤 아래 7개 WIP는 합계55파일 +7589/-709다(문서·시험 포함).
소스의 양을 진전으로 평가하지 않는다. 각 변경은 컴파일·시험·변이 미실행이며 유지의 타당성도
다음 통합 검증에서 판정해야 한다. 검증된 결과가 없다는 이유로 사용자 변경을 삭제하지도 않는다.

| 커밋 | 작성한 변경 | 현재 지위 |
| --- | --- | --- |
| `7f402aba5` | owned completion 저장소·예약 기반 | 실제 전체 전달 연결 미완 |
| `bcbadf101` | committed 송신물 불변 보존 | 작성된 후보 |
| `d8fff7d27` | 전달 슬롯과 결과 보존 공간 분리 | 전체 byte/RSS 예산 증명 아님 |
| `2b1d1d539` | PREFILL 수용의 준비/확정 원자성 | 실제 소비 회귀 작성, 미실행 |
| `658c9cded` | broker/adapter/node terminal 거부의 원본 반환 | raw Event 소유 보존 후보, claim 이관 아님 |
| `f5aa09675` | 직접 응답 FIFO·알림 경계 | 작성된 후보 |
| `6fe10eb10` | 요청 입력의 불변 공유 | 복제 축소 후보, 성능/RSS 효과 미측정 |

이후 중단한 후보는 **독립 correlation의 completion front를 실제 목적지 슬롯 확보 후 전달**하는
중립 EventNode/broker 수정이다. 같은 `(source, correlation)` 순서는 유지하며 payload를 해석하지
않는다. `NodeAdapter`/mailbox·Llama 위임·broker receipt·terminal 원본 보존·실제 actor wrapper까지
연결한 코드와 회귀20개(broker12/mailbox4/EventNode4), actor 예방 witness가 작성돼 있다.
이것은 정적 연결 확인뿐이며 cap1 통과를 관측한 것이 아니다. 계약의 해당 절도 **미검증 후보**다.
원본 입력14개·출력6개·SESSION_READY8개·cap1/cap8·외부 dequeue0 최종 판정은 유지 대상이다.

### 0.4 전진이 불명확해진 원인과 중단한 접근

- 실제 실패 하나를 닫기 전에 범용 소유형/메모리 이관을 계속 선행 조건으로 추가했다.
  유용할 수 있는 기반 변경도 소비 경로 통과가 없으면 현재 반례의 해결로 셀 수 없다.
- 국소 함수·ACK GREEN과 전체 actor 진행성을 구분하지 못하면 같은 종류의 결함을 뒤늦게 찾는다.
  지금은 후자를 RED로 고정했지만, 그 뒤 통합 통과를 얻지 못한 채 WIP가 누적됐다.
- 긴 시간순 기록마다 “다음”이 바뀌어 현재 상태와 완료 경계가 흐려졌다. 현재 지시는 §0 하나로
  모으고 이력은 보존하되 실행 순서로 재사용하지 않는다.
- 마지막 수정·예외 없음·세 번 안의 성공은 증명 없이 약속할 수 없다. 대신 반례·가정·소유자·
  종료 조건을 실행 전에 고정하고, 예상 밖 실패를 기대값 완화나 새 라운드 이름으로 감추지 않는다.

### 0.5 재개 후 해야 할 일 — 기존 B 단계의 우선순위

**지금은 실행하지 않는다.** 재개 지시 후 아래 순서로 진행하며 새 선행 리팩터를 덧붙이지 않는다.
단계 전체의 종료 조건은 §6, 시험 상세는 검증 규약, 층별 책임은 격리 계약이 소유한다.

1. **현 후보의 판정부터(B1/B2).** HEAD/변경·7개 WIP와 신규 후보를 대조하고, 실제 대기 고리에서
   어떤 간선만 제거하는지 설명한다. 슬롯 증설·SESSION 전용 우회·실패 입력 삭제 없이 같은
   actor cap1 정상 진행과 cap8 대조, 순서/원본/중복/terminal 회귀를 봉인한다. 보조 API만 더 만드는
   작업으로 바꾸지 않는다. 검증 3라운드 제약은 **2회 사용·1회 미사용**으로 유지한다.
2. **통합 안전성의 잔여를 닫기(B1/B2/B3).** 해당 cap1 통과도 일반 교착 자유가 아니다.
   동일 순서 영역의 순환, 취소/Close/Drain, 포화 중 필수 결과·반환 공간, 1ms 재시도를 대체할
   capacity wake, queue/retained/byte 수명, 중복·재접속·불명 실행의 수렴을 실제 소비 경로에서
   판정한다. 아직 선언되지 않은 native 결과 bound와 remote acceptance를 임의 숫자로 대신하지 않는다.
3. **수용·비행 budget 완결(B3).** pending/토큰/byte/KV 셀을 구분하고 실제 메모리에서 예약한다.
   prepared를 포함한 다중 노드 예약과 edge row/byte credit의 유일한 소유자를 정한다.
   공간 부족은 제한된 대기/명시 거절, 반환·취소·timeout은 멱등 정산이어야 한다. 슬롯 수나 ID 여력을
   host RAM/KV 예산으로 부르지 않는다.
4. **배치 정책 완결(B4).** 같은 상태 전이를 사용하는 실제 worker/reference에서 전체 runnable
   재선택, 일반/등폭/atomic 제약, decode 의존성, chunked prefill, 요청별 공정성을 검증한다.
   `min_batch_rows`·`max_open_batches`·`max_issue_rows`·다중 prefill fragment를 최적값으로
   전제하지 않는다. 준비됐지만 미발행한 행은 KV/credit/shape/의존성 등 이유별로 설명한다.
5. **실제 실행과 업데이트 경계 완결(B5, 안전성 작업과 필요한 범위만 병행).** 제품 LOAD의
   model/build/ABI/layout/capability 강제, 실제 장치·host fallback 결속, 선언 backend conformance,
   native private/common 간접 include/link·public signature 격리를 확인한다. 순수 원장/정책은
   llama private 타입을 보지 않고 upstream 적응은 허용 compat 모듈에서 끝내야 한다.
6. **그 뒤 성과 판정(B6/B7/B8).** `S:\models`와 승인된3090×2에서 VRAM-only 모델의 강한 연속
   웨이브·정상 응답을 먼저 증명한 뒤 RAM 오프로딩 모델로 확장한다. 토폴로지를 고정한 paired
   A/B·holdout에서 유효 생성 TPS와 SLO·GPU 계산을 함께 판정하고 soak/fault·재시작 없는 반복을
   통과시킨다. 현재 GPU 두 장은 한 물리 호스트이며 최종 다중 컴퓨터 증명은 별도 자원이 필요하다.

영속 KV/스냅샷(K 분기)은 기반 코드가 있으나 목표 계약 전체는 미완이다. 워크로드가 요구하는
기능부터 장애 게이트와 연결하며 과거 U/P 전체를 다시 직렬 선행 조건으로 만들지 않는다.
단일 GPU/replica 비교는 비용 진단용일 뿐 용량 때문에 필요한 분산 노드를 없애는 승인 기준이 아니다.

### 0.6 다음 세션의 검증·보고 제약

#### 실기 측정은 3090×2에서만 한다 (2026-09-09 사용자 지시, 상시)

- **측정은 `m42-server2`(RTX 3090 ×2)에서 한다.** 개발 PC(`hikaTR`, 3090 + 4080)에서 측정하지 않는다.
  카드 구성이 다르므로 두 기기의 값을 같은 열에 섞지 않는다. 개발 PC는 빌드·문서·시험 전용이다.
- **SSH 세션은 원격의 `S:`를 보지 못한다. 그러나 우리가 그 호스트에서 띄우는 앱은 본다.**
  드라이브 매핑은 로그온 세션마다 별개라서, 원격에 로그온돼 있어도 SSH 세션의
  `Test-Path 'S:\models'`는 False다. 로그온 여부의 문제가 아니다.
  그래서 에이전트를 **대화형 예약 작업**으로 띄우고(`remote-agent.mjs start`), 그 프로세스가
  연 `S:`를 stage 서버가 물려받는다.
- **모델 경로는 이 PC의 `S:` 경로와 같다.** 즉 시나리오에 적힌 `S:\models\...`를 그대로 쓰면 되고,
  경로를 원격용으로 바꿔 적을 필요가 없다. 여기서 `S:`가 보이면 거기서도 같은 문자열로 열린다.
- 확인할 일이 있으면 SSH로 `Test-Path`를 물어 판단하지 않는다. SSH가 못 보는 것은 정상이며
  적재 가능 여부의 근거가 아니다.
- 호스트가 로그인 화면이면 대화형 작업이 돌지 않는다. 그때만 예외적으로 가중치를 원격 로컬 디스크에
  옮기고 `run.mjs --model`과 `remote-agent.mjs --no-session`(S4U)을 쓴다. **기본 경로가 아니다.**

- 새 세션은 §0와 검증/격리 계약을 먼저 읽고 현재 HEAD와 비교한다. **사용자의 재개 지시 전에는
  마지막 검증 회차를 실행하거나 구현을 더하지 않는다.** 지금 중단을 BLOCKED/최종 완료로 오기하지 않는다.
- 검증할 소스·입력·판정·실패 원인별 제거 변이를 먼저 봉인한다. 실행 중 같은 checkout을 편집하지
  않는다. 변이는 독립 복사본과 실제 재컴파일/EXE 결속으로 검증한다. 컴파일 오류/timeout은 의도한
  불변식 검출로 세지 않는다. 미실행·ignored·feature 제외를 passed에 넣지 않는다.
- 전체 결과와 cap1 정상 진행, 정상 대조, 순서·중복·원본 보존, 수정 제거 시 실패를 함께 보고한다.
  예기치 않은 실패가 나오면 그 원인을 먼저 기록한다. 상한·골든·judge·입력·기아 기준을 완화하거나
  세 라운드 예산을 다시 시작하지 않는다. 현재 후보가 반드시 통과한다고 전제하지 않는다.
- 각 체크포인트는 **검증됨/미검증**을 제목과 상태에 구분한다. 필요한 운영 코드·회귀·소유 문서를
  전체 보존하고 작업 트리 잔여를 확인한다. 모델·바이너리·빌드·임시 원문은 ignore하며, clean Git이
  테스트 통과나 장기 증거 보존을 뜻하지 않는다.
- 최종 성과 보고는 정상 프롬프트·응답 전문, 강한 겹치는 웨이브, 소스/바이너리/모델/placement
  신원, 오류·정산·credit 및 유효 TPS/SLO/GPU 자료가 함께 있어야 한다. 로컬 안전성 통과만으로
  “최적의 배치 구현 완료”라고 쓰지 않는다.

### 0.7 인플라이트 파이프라인 성능 작업 순서

이 절은 **처리량을 올리는 작업의 순서**만 소유한다. 안전성 잔여는 §0.5, 시험 판정은 검증 규약이
계속 소유한다. 아래 수치는 `20260907T093641Z-5eeea50c`(원격 3090×2, 35B 2-stage, VRAM-only,
resident 32, ctx 2560/seq)의 보존 span을 요청·집단 단위로 다시 연결해 옮겼다. 원자료의 기준은
`2ed9b71d4` + 보존 dirty diff이고 native patch set은 `3cfc636181e4`다. **`0025` 이후의 재측정이 아니다.**

> **2026-09-09 교정 기록.** 이 절의 최초 판(`daae4c8fd`)은 네 가지를 틀리게 적었다.
> ① 이미 철회된 상관관계(r=0.891)를 폭 정책의 근거로 인용했다. ② 서로 다른 집합을 곱한 값을
> 항등식이라고 적었다. ③ RPC 구간 겹침을 GPU 동시 계산으로 읽었다. ④ 의도된 정책의 결과를
> 결함으로, 그 위에서 계산한 191 TPS를 기대 이득으로 적었다. 아래는 그 교정판이다. 틀린 판을
> 지우지 않고 무엇이 왜 틀렸는지 남기는 것은 09-03 증거 문서와 같은 규율이다.

#### P-0. 현재 처리량이 무엇으로 분해되는가 (측정됨)

| 항목 | 값 |
| --- | ---: |
| release까지의 벽시계 T | 325.868 s |
| 물리 batch (decode / prefill) | 2,459 (2,410 / 49) |
| decode 행 / 생성 token | 38,084 / 38,148 |
| decode batch/s × decode 평균 행 | 7.395633 × 15.802490 = **116.869 decode row/s** |
| 생성 token / T | **117.066 TPS, 품질 판정 전** |
| decode head RPC 평균 / tail RPC 평균 | 81.52 ms / 89.68 ms |
| decode 유휴 평균 | 49.36 ms |

**이것은 항등식이 아니라 계수의 재배열이다.** `38,084 / 325.868`을 `(2,410 / 325.868) × (38,084 / 2,410)`로
쓴 것뿐이며, 왜 그 속도가 됐는지는 증명하지 않는다. 이전 판의 `7.55 × 15.80 = 119`는 전체 batch/s에
decode 전용 평균 폭을 곱한 값이라 어느 집합의 속도도 아니다. 사용하지 않는다.

`stage_ms + idle_ms` 합은 322,479 ms(평균 131.142 ms), `T / 2,459`는 132.521 ms로 3,389 ms가 남는다.
정수 ms 절삭과 유휴 종료~native 시작 사이의 준비 구간이 섞인 차이다. 두 값을 같다고 쓰지 않고,
이 차이를 별도 병목이라고 부르지도 않는다.

오프로딩 대조(`20260907T102817Z-8438348c`, 전문가 CPU)는 stage p50 233 ms, 주기 354.3 ms, 43.19 TPS다.
**prompt template가 달라 placement만 바꾼 인과 비교가 아니다.** 두 경로를 같은 항목으로 묶지 않는다.

#### P-1. 폭에 대해 실제로 아는 것 — 인용해 온 상관관계는 이미 철회됐다

이전 판은 `worker/drive.rs`의 r=0.891(겹침)·r=-0.357(폭) 주석을 근거로 폭을 목표에서 제외했다.
그 주석의 원자료인 [09-03 기록](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-03-load-and-batching.md)은
바로 아래에서 그 해석을 취소한다: `Everything in the paragraph above is backwards.` 후속 절 제목은
`The correlation was reverse causation (2026-09-04)`다.

- `P4_STAGED_MAX_ISSUE_ROWS`로 폭을 직접 제한한 8회 교차 실험에서, stage 겹침은 95.4%까지 오르고
  GPU 사용률도 가장 높았는데 total row/s는 544.25 → 198.10으로 떨어졌다.
- 폭을 통제하면 부호가 뒤집힌다: 폭 **+0.898**, 겹침 **-0.060**.
- 적합: **tail step = batch당 34.2 ms + 행당 1.051 ms.** 폭 98까지도 곡선은 계속 오르며, 넓은 batch가
  좁은 batch보다 행당 3.4배 효율적이다. 겹침이 오른 것은 바쁜 stage가 고정비를 반복해 낸 결과다.
- 그 고정비의 내역도 이미 측정돼 있다. tail에서 `llama_decode` 40.1 ms보다 **sampling 46.9 ms가 크다.**
  행당으로는 transformer 0.11 ms 대 sampler 0.29 ms로 샘플러가 2.7배다. 어휘가 최소 249,157이고
  `common_sampler_sample`이 행마다 그 크기의 후보 배열을 단일 스레드로 만든다.
  계측은 지금도 코드에 있다(`server_physical.cpp:34`, `P4_STAGED_TRACE_STEP`).

**따라서 결론이 반대로 바뀐다.** 겹침·UBATCH 채움률은 여전히 목표가 아니지만, 폭을 기각해서도 안 된다.
**지렛대는 batch당 고정비이고, 이를 줄이는 길은 폭으로 상각하거나 샘플러 자체를 고치는 것이다.**
그 주석은 `2bc8f93cd`에서 09-04 결과로 교체했다. 코드에 남은 철회된 인용은 없다.

#### P-2. 먼저 복구해야 하는 측정 신뢰 (§0.0의 1~3과 같은 항목)

성능 판정을 시작하기 전에 끝낸다. 모두 **성능 실험의 결과를 못 읽게 만들기** 때문이다.

1. ~~시험 전용 접근자로 HEAD lib test 컴파일 복구.~~ **2026-09-09 완료** (`2e865ff51`).
   E0594로 종료 101·실행 시험 0이던 상태에서 **1,361 passed / 0 failed / 7 ignored**로 회복했다.
   회귀 없이 스케줄러를 만지지 않았다. 남은 2~3번이 성능 측정의 잔여 선행조건이다.
2. ~~`run/mod.rs`가 UNLOAD 전에 `InferenceResult::error`와 부분 결과·정산 상태를 보존하도록 고치고,
   primary/cleanup 오류를 따로 기록한다. `run.mjs::stopChild`의 신호 종료 오판도 고친다.~~
   **2026-09-09 완료** (`d8203e373`).
   - teardown을 분리하고 반환형을 `Result`가 아닌 `Option<String>`으로 두었다. `?`로 전파할 수 없으므로
     **원래 결함을 다시 쓰면 컴파일 오류**다(E0277로 확인). 시험이 아니라 타입이 막는다.
   - `error`(실행 자신의 최초 실패)와 `cleanup_error`를 분리했고 산출물은 항상 기록된다.
     **UNLOAD guard는 그대로다.** 정리 실패는 여전히 실행을 실패시키며, 양성 대조를 둔 시험이 이를 고정한다.
   - `stopChild`: `kill()`은 신호를 보내므로 정상 종료도 `exitCode`가 null이다. 이 환경에서
     `code=null signal=SIGTERM`으로 재현했다. **모든 정상 정지가 정지 실패로 보고되어**
     `agent_stopped=false`가 실행을 실패시키고 있었다.
   - 시험 7개(event-drive 3 + stopChild 4). 각각 변이로 판별력을 확인했다. 워크스페이스 1,364 passed.
3. ~~`pressure`(resident 256) 재판정.~~ **2026-09-09 판정 완료.**
   수정 직후 **3090×2에서 두 번** 재실행했다(가중치 스테이징본, 그리고 시나리오 원래의 `S:` 경로).
   두 실행 모두 산출물이 남았고 두 오류가 분리됐으며 같은 실패를 재현했다.
   - `error` = **`stage control batch total receipt budget is exhausted`**.
     `validate_control_batch`의 제품 호출자는 `release.rs:324`와 `settlement.rs:246` 둘뿐이므로,
     걸린 것은 **해제·정산 경로 자신**이다.
   - `cleanup_error` = `unload is busy`. 그 작업 스냅샷은 **비행 작업이 전부 0이고**
     `active_owners`/`active_frontiers`만 남는다(두 실행에서 224와 256). 512 요청 중 96개/64개가
     release member를 받고도 `released`는 **양쪽 다 0개**다. 보존된 09-07 실행의 `224/256`과
     같은 범위다.
   - **판정: UNLOAD 거부는 원인이 아니라 결과다.** 방치된 상태가 새는 것이 아니라 **해제가 용량
     한계에 막혀 시작되지 못했다.** 한계는 `ownership.rs`의 `MAX_CONTROL_BYTES`(1 MiB, 행마다
     최악 응답을 예약)와 `MAX_RECEIPT_BYTES`(64 MiB, 누적 상한)의 조합이며, 제어 batch는 **63건까지
     허용하고 64건부터 거부한다**(보관된 receipt가 없을 때의 경계). resident 256의 전폭 해제는
     구조적으로 이를 넘는다.
   - **정정: `released_count=0`은 "해제가 실행된 적이 없다"는 뜻이 아니다.** head는
     `worker/effects.rs`의 `CommittedEffect::Release`에서 자기 stage의 해제를 실제로 수행한 뒤
     전달하며, 그 경로는 sequence 하나짜리 개별 검사를 쓰므로 batch 합계 검사에 걸리지 않는다.
     옳은 진술은 **전체 stage의 해제 완료가 확인되지 않았다**이다. UNLOAD 거부가 실은 작업
     스냅샷도 그 노드 하나의 상태이며, 어느 stage가 어디까지 진행했는지는 그 산출물로 판정할 수 없다.
   - 미확정: **그 실행에서** 거부된 명령 종류와 batch 폭(산출물에 없다. 63건은 상수와 wire 인코더에서
     확정한 상한 회계의 경계이지 그 실행의 폭이 아니다), 완료 수와 owner 수가 실행마다
     흔들리는 이유, 보존된 09-07 실행이 같은 경로를 밟았는지 여부. [증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-measurement-trust-recovery.md)
   - **따라서 §0.5 3번(수용·비행 budget)이 resident 상향의 선행조건으로 확정됐다.** 예산을 세우기
     전에 resident를 올리면 같은 벽에 다시 닿는다. P-4의 resident 축은 그 뒤에 온다.
4. ~~`drive.rs:130`의 철회된 상관관계 인용을 09-04 결과로 교체한다.~~ **2026-09-09 완료** (`2bc8f93cd`).
   주석만 바뀌었고 `cargo check --lib` 통과.
5. **제어 응답 예산을 명령별 상한과 연결.** 코드·시험·변이·**실기 `pressure` 재판정 2026-09-09 완료**.
   - 예산 검사가 새 제어마다 실제 응답이 아니라 `MAX_CONTROL_BYTES` 1 MiB를 예약하고 있었다.
     이제 명령이 **자기 계약이 증명하는 상한**을 선언하고 개별 검사와 batch 합계 검사가 같은 값을 쓴다.
     RELEASE는 요청과 동일한 응답을 요구하므로 요청 길이, SETTLE은 `physical_capacity × 4 + prefix + 4`다.
     `commit_control`은 도착한 응답의 실제 길이로 정산한다. 상한(1 MiB·64 MiB)은 올리지 않았고
     resident도 내리지 않았으며 공통 코어의 예산 구조도 건드리지 않았다.
   - 거부 문구가 member 수·기존 보관량·추가 예약량·요구량·상한을 싣고, 호출자가 명령 종류와 폭을
     앞에 붙인다. 다음 실행부터 거부된 batch를 산출물만으로 특정할 수 있다.
   - 시험 3개 추가(생산 상한에서의 전폭 batch 1개, 실제 소비 경로 2개)와 기존 2개 갱신.
     워크스페이스 **1,367 passed / 0 failed / 7 ignored**. 변이 3회차를 분리된 worktree에서
     재컴파일 sha256과 함께 확인했다(개별만/합계만/둘 다 → 2·3·4개 실패).
   - **실기 재판정: 3090×2에서 두 번 모두 512/512/512 통과**(`20260909T034439Z-4ce8e2b1`,
     `20260909T035149Z-9014d441`). `error`·`cleanup_error` 모두 `null`이므로 idle UNLOAD도 통과했다.
     512개 요청이 물리 슬롯 256개를 슬롯당 2건씩 쓰고 incarnation 1–512로 전부 해제됐다.
     staged 서버·DLL·launcher 해시는 실패 실행과 같고 **바뀐 것은 어댑터 바이너리 하나다.**
   - **검증 범위는 RELEASE다.** 두 실행 모두 Verify/Replay 행이 0이고 산출물에 정산 기록이 없다.
     SETTLE 배치 경로의 근거는 소비 경로 시험과 변이 수준에 머물며 실기 판정은 아직이다.
   - **성능 승인이 아니다.** 수정 전 `pressure`는 완주한 적이 없어 비교할 기준선이 없다.
     이 실행의 TPS를 개선폭으로 인용하지 않는다.
     [증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-control-receipt-budget.md)

#### P-3. 무엇을 기다렸는가 — 30행과 2행 집단의 반복

이전 판은 "ready 86행을 두고 쉬었다"고 적었다. **틀렸다.** `ready_rows`는 다음 계획 시점의 스냅샷이고
남은 prompt token 전체를 포함하므로, 대기 내내 발행 가능했던 행 수가 아니다. 보존 span을 요청까지
연결하면 실제 모습은 다르다.

| decode 폭 | 횟수 | head RPC | tail RPC | 직전 유휴 |
| ---: | ---: | ---: | ---: | ---: |
| 2행 | 1,077 | 37.31 ms | 41.65 ms | 0.54 ms |
| 30행 | 951 | 126.63 ms | 138.05 ms | 101.38 ms |

**resident 32개가 30개와 2개 두 집단으로 갈려 번갈아 발행된다.** 30→2 전환 945회의 사이 유휴는
평균 0.46 ms, 2→30 전환 943회는 평균 102.16 ms다. 평균 15.80행은 균일한 16행 batch를 뜻하지 않는다.

1. 50 ms 이상 유휴 1,165개가 전체 유휴의 **98.85%**다. 그중 decode 1,164개는 발행 시점
   `ready_rows == 발행 행`이다(양의 잔여는 1,165개 중 1개). **남겨 둔 행은 없었다.**
2. 이 1,164개는 자기 요청들의 마지막 선행 tail RPC 종료 뒤 평균 **1.36 ms**, tail forward 뒤 평균
   **0.88 ms** 만에 다음 계획을 시작했다. 5 ms 이내가 1,162개다. **스케줄러가 늦은 것이 아니라
   기다릴 것이 있었다.**
3. head 종료~다음 계획 공백 합계 120.941 s 중 **118.613 s(98.08%)**가 tail RPC와 겹친다.
   **GPU 계산 겹침이라는 뜻이 아니다.**
4. 같은 공백의 **105.188 s(86.97%)**에는 resident **32개 전부**가 head에서 decode를 발행했으나 각자의
   tail이 아직 끝나지 않았다. `state.rs:351`이 `outstanding > 0`인 요청의 다음 행을 발행 불가로
   만들기 때문에, 이 구간에는 발행할 행이 실제로 없다.
5. 첫 batch를 뺀 2,458회 중 **2,454회**가 직전 batch의 tail 종료 전에 다음 계획을 시작했다.
   "앞 batch 반환 전 발행"은 이미 하고 있다. 후보로 다시 적지 않는다.

**1순위 가설: 큰 집단이 tail에 있는 동안 작은 집단을 head가 먼저 끝내고, 큰 집단의 다음 token을
기다리는 비대칭 반복.** 아직 없는 증거는 모든 공백에서의 admission/eligible 상태 전량과 native 내부
비용이다. 남은 공백을 전부 스케줄러 결함으로 분류하지 않는다. 다음을 한 실행의 같은 시각축에 남긴다.

- 요청별 `선행 tail 종료 → forward → head 수신 → 정산 → eligible → 다음 native 시작`.
- 모든 미발행 구간의 eligible/admitted/in-flight 수와 거절 사유. `idle_gated=0`은 두 gate의 거절이
  없다는 뜻일 뿐 모든 발행 불가 사유를 계측한 값이 아니다.
- 기존 STEP trace로 폭 2 / 중간 / 30에서 head·tail을 parse·decode·sampler·encode로 나눈다.
  **새로 만들 것이 없다.** 타이머는 `server_physical.cpp:34`에, 스위치는 `remote-agent.mjs:75`의
  `--step-trace`(→ `P4_STAGED_TRACE_STEP=1`)에 이미 있다. 1 ms 단위 원인을 보려면 ms 절삭보다
  세밀한 단위와 trace 오버헤드 대조가 함께 필요하다.
- 올바른 ChatML template와 정상 응답 검사를 고정한 35B VRAM 기준선을 새로 봉인한 뒤에 잰다.
  현재 117.07은 heuristic judge 60/64인 품질 판정 전 값이다.

6. **실패한 실행의 부분 결과 보존.** **2026-09-09 완료.**
   - 09-09 포화 실험의 실패 4회(2-stage 적재 거부 2, 노드 중복 1, 4-stage 추론 중단 1)가
     `artifact.json`을 남기지 못했다. `inference::drive`가 루프 안의 실패를 `?`로 내보내 모아 둔
     요청·출력·완료·관측이 통째로 사라졌고 `assemble`에 닿지 못했다. 그래서 **그 실행들이 어디까지
     승인했는지, 최초 원인이 무엇인지 지금도 알 수 없다.**
   - 이제 제출이 시작된 뒤의 실패는 결과다. 최초 오류는 유지되고, 승인된 것은 보존되며, 거절된
     이벤트는 받아들여지지 않고, 실행은 여전히 실패(`passed=false`, 종료 코드 1)로 끝난다.
     `submission`(delivered/uncertain), `evidence_missing`, `submissions` 요약을 산출물에 넣었다.
   - 실제 소비 경로에 연결 절단·deadline 만료·잘못된 후속 이벤트를 각각 주입하는 시험,
     그 위에 UNLOAD 거부를 얹는 시험, wave 도중 쓰기 실패로 delivered/uncertain/unsubmitted를
     가르는 시험, 그리고 **로컬 TCP peer에 실제 CLI 바이너리를 붙여 `execute → teardown →
     JSON 기록 → 종료 코드 1`을 고정하는 통합 시험.**
     귀속은 증거를 따른다 — 증거가 완전하면 실패·미해제와 무관하게 행 수를 귀속하고,
     `evidence_missing`이 `null`이 아닐 때만 0으로 남는다.
   - **09-09 실패의 최초 원인은 여전히 미확정이다.** 이 수정은 다음에 같은 일이 생기면 판정할
     자료가 남게 할 뿐이며, 재시도 성공으로 닫지 않는다. 새 시험의 주입 지점이 그 실패와 같은
     지점이라는 증거도 없다.
     [증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-partial-result-preservation.md)

7. **계획 모드의 recurrent 할당 수정.** 코드·적용·컴파일 **2026-09-10 완료**, 실기 게이트 **통과**
   (r96·r256에서 계획 pass RS 0.00 MiB, r256 admitted·완주, 넘치는 구성은 계속 거부 —
   [릴리즈 게이트](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-release-gate-v0.9.0.md)).
   **해당 2B·35B의 각 stage host/device 계획=실제는 마감 감사에서 대조 완료.**
   r160 및 다른 모델/backend 무회귀는 다음 버전. 새 binary r256 완주는 OS 재시작으로 재판정이 남았다.
   - 상류 `llama_kv_cache`는 `no_alloc`에서 크기 0 dummy 버퍼를 쓰고 `memory_breakdown()`도
     정렬을 반영한 예상 크기를 보고한다. `llama_memory_recurrent`는 **둘 다 하지 않아** 계획을
     세우는 동안 recurrent 상태를 실제로 할당하고, 그 뒤 줄어든 `free`를 그 비용이 포함된
     `required`와 비교했다. compat 패치 `0026-noalloc-recurrent-residency.patch`로 두 곳을 맞췄다.
   - **fit 검사를 없애거나 `free`를 보정하지 않았다.** 공간이 실제로 부족한 구성은 계속 거부한다.
   - 26개 패치 적용·경계 검사·준비 트리 diff 해시(`f37f181c…`)·ABI 심볼 통과, CPU Release에서
     `llama.dll` 링크 성공. 고정 pin checkout은 손대지 않았다(작업은 분리된 worktree).
   - **다음 단계이자 완료 조건:** 계획용 RS 실제 할당 소멸을 backend 초기화 비용과 구분해 확인,
     r96·160·256의 host/device별 계획=실제 대조, attention·recurrent·hybrid 무회귀,
     **실제로 부족한 구성의 거부 유지**, 수정 뒤 r256의 실제 적재·최대 메모리·추론·UNLOAD 별도 통과.
     [증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-noalloc-recurrent-residency.md)

#### P-3d. 결정된 작업 순서 (2026-09-10)

| 순서 | 할 일 | 완료 조건 |
| ---: | --- | --- |
| ~~1~~ | ~~실패의 부분 결과 보존~~ | **완료**(P-2 6번). 귀속은 증거를 따르고, 실제 CLI가 산출물을 쓰고 1로 끝나는 것까지 고정 |
| 2 | **no-alloc recurrent 동작 검증** | 위 7번의 완료 조건 다섯 가지 |
| 3 | **B2/B3 수용·반환 예산 연결** | 첫 수용 쓰기 이전부터 pending 개수·바이트·토큰과 필수 반환 공간 확보. 거부·취소·실패에서 원장·예약·출력 권위 보존과 정확히 한 번의 정산. 포화 중에도 기존 요청은 반환까지 진행 |
| 4 | **정상 응답 기준선과 원인 계측** | 완결 응답과 고정 도착 패턴을 봉인하고, 발행 불가 사유별 대기와 STEP trace의 parse/decode/sample/encode를 함께 남긴 뒤 반복 A/B |
| 5 | **근거가 있는 최적화** | H5대로 같은 모델·토폴로지·resident·KV·입력에서 정책 하나만. 8쌍 반복과 holdout, 유효 TPS 중앙 개선 ≥5 %, 신뢰구간 하한 >0, TTFT·ITL SLO |

**resident 상향은 3번 뒤에 온다.** r256 적재 성공은 서비스 승인이 아니다.
**`state.rs`의 `outstanding > 0` 검사는 유지한다** — 다음 decode는 앞 토큰의 결과를 필요로 하므로
이 검사를 없애 적격 행을 늘리는 것은 최적화가 아니라 의존성 위반이다.

#### P-3e. 이번 버전 마감 — 로컬 완료, 외부 실기 BLOCKED

사용자 결정은 v0.9.0, 0026은 필수 실기 통과 시 포함이다. 초기 게이트 통과 뒤 마감 감사에서
분석 도구·공식 Release builder·바이너리 재빌드·원자료 보관을 보완했다(`3302591fc`).

| 마감 단위 | 상태 |
| --- | --- |
| 코드·회귀·빌드 | 완료. 회귀 7개와 독립 변이, workspace 1,374/0/7, Node 전체 146/0, CTest 15/15 |
| 새 binary 실기 | smoke·pressure·35B r96 통과. r256은 Windows Update 계획 재시작으로 중단됐고 부분 artifact 보존 |
| 문서·원자료·패키지 | 후보로 보존. PLAN/ACTUAL 과소 보고 오독 철회, 10개 실행 원자료 포함, 불완전 게이트 명시 |
| 정식 tag·릴리즈 승인 | **BLOCKED**: 42mob 로그인 복구 뒤 r256 재판정·must_refuse → 증거 갱신 → annotated v0.9.0 |

기존 미공개 tag는 archive ref와 원자료로 보존한다. 새 후보를 정식 통과로 바꾸거나 초기 바이너리 검증을 승계하지 않는다.
원격 OS 업데이트·로그인·드라이버 설정은 변경하지 않았다. push는 별도 권한이다.
다음 버전의 B2/B3·정상 응답 기준선·원인 계측·최적화를 이번 마감에 추가하지 않는다.

#### P-3b. `pressure`의 실측 기준선 (2026-09-09, 검증됨)

`pressure`가 처음 완주했으므로 이제 이 시나리오에도 실측 기준선이 있다. 두 실행에서 재계산했고
숫자는 [증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-pressure-measured-baseline.md)가
소유한다. 재현은 `node test/benchmarks/p4-4node/measure-run.mjs <run dir>`이다.

| 지표 | 실측 | 목표 대비 |
| --- | --- | --- |
| 응답 | 512/512가 한국어 TypeScript 설명, 반복 붕괴 없음. 그러나 **512/512가 `max_tokens` 200에서 잘렸다** | **정상 응답 증거로 쓸 수 없다.** 완결 예산·stop 조건을 갖춘 별도 실행이 필요하다 |
| 생성 TPS | 382.15 / 401.75 | 비교 기준선 없음(이전에 완주한 적이 없다) |
| UBATCH 채움률 | **15.95 % / 17.02 %** (폭 평균 81.65·87.13 / 512) | 관측값. 채움률은 목표 지표가 아니다(P-3c) |
| decode 폭 최대 | **160** (resident 256인데) | 천장이 정책이 아니다 — `idle_gated=0`, `ready_rows_left` p50·p90 0 |
| kernel 활성 창 평균 | gpu0 18.8 %, gpu1 28.2 % (전력 93 W / 177 W) | kernel이 올라와 있던 시간 비율. SM 점유율이 아니므로 이것만으로 포화 여부를 말하지 않는다 |
| stage 점유 | node0 40 %, node1·2 약 30 %, **tail 76.6 %** | tail이 제약 |

**폭 천장의 위치가 바뀌었다.** 09-04 35B 분석은 "남겨 둔 행이 없었다"였고 여기서도 같지만,
이번에는 `ready_sequences` 자체의 최대가 160이다. `state.rs`의 `phase_within`이 decode에서
`outstanding > 0`인 요청을 제외하므로, 4 stage·`depth_mean` 3.18에서 resident의 상당수가 항상
비행 중이다. **다음 측정은 발행 불가 사유별 시퀀스 수를 직접 기록해 이 분해를 확정한다.**
`ready_sequences` 최대 160은 결과이지 원인 분해가 아니다.

#### P-3c. 포화·사용률 실험 10회 (2026-09-09, 검증됨)

한 번에 하나씩 바꿔 3090×2에서 열 번 돌렸다. 숫자는
[증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-saturation-and-utilisation.md)가
소유한다. 열 실행 모두 512 완료·512 해제, 두 오류 모두 `null`이다.

- **2B와 35B에서 kernel 활성 비율이 크게 달랐다.** 2B는 21.4~32.9 %, 35B는 36.8~50.7 %, 전력은
  99~160 W 대 175~213 W다. 다만 두 구성은 구조·양자화·flash attention·KV 형식·resident가 함께
  다르므로 **"모델 크기가 사용률을 결정한다"는 아직 검증하지 않은 가설**이다.
- **한 카드 한 stage.** resident를 고정하고 분할만 바꾸면 4 stage가 33 % 느리다(275.35 대 183.00).
  이 단회 실행에서 2 stage를 **기준선 후보**로 삼는다. 4 stage가 resident 256에서 좋아 보였던 것은
  분할이 아니라 resident 때문이다.
- **`min-batch-rows 64`는 채택 후보에서 제외한다.** 같은 입력에서 채움률을 9.09 → 25.14 %로 올리고
  TPS를 19 %, kernel 활성을 40 % 떨어뜨렸다(겹침 73 → 3 %, 깊이 3.29 → 1.03).
  **채움률은 목표 지표가 아니다.**
- **도착 패턴이 성능을 크게 바꾼다 — 입력 민감도로 분류한다.** resident 96 → 160에서도 decode 폭은
  32와 64 둘뿐이었고, 버스트를 키우면 폭이 넓어졌다(4건/125 ms 223.10 → 32건/1 s 289.79 →
  64건/2 s **313.80** → 128건/4 s 302.49). **64 → 128건에서 폭은 51.12 → 53.65로 늘었는데 TPS는
  떨어졌으므로 폭만으로 설명되지 않는다.**
- **적재 판정이 구성을 과도하게 거부한다.** `stage_memory_plan.cpp`는 계획용 모델을 `no_alloc=true`로
  만들지만 recurrent 버퍼는 계획 중 실제로 할당된다. 그 뒤 줄어든 `free`를 recurrent를 포함한
  `required`와 비교한다(5회 계획에서 `free + RS = 22.76 GiB`). **staged 서버이므로 이 세션에서
  고치지 않았다.**

**313.80 TPS는 시험한 조건 중 단회 최고치이며 하드웨어 한계도 최적값도 아니다.** 앞서 적었던
"35B는 약 276 TPS에서 포화"는 이후 실행이 반박했으므로 철회한다. 성공 11회 전부가 요청당 200토큰
`length` 종료인 **고정 길이 부하시험**이고, 기본 r160의 **TTFT 중앙값은 93.9 초**다(수용 대기만의
측정이 아니므로 admission 병목으로 귀속하지 않는다). 정상 서비스 승인이 아니다.
다음 순서는 위 증거의 마지막 절이 소유한다.

#### P-4. 귀속 결과가 정당화할 때만 하는 변경

| 후보 | 근거가 될 측정 | 상태 |
| --- | --- | --- |
| decode 집단 균형화 (30+2 → 균등) | P-3이 비대칭 반복을 1순위로 확정 | **첫 후보.** 아래 크기 추정 참조. `MAX_ISSUE_ROWS`는 prefill 폭까지 제한하므로 그대로 쓰면 효과를 분리하지 못한다 |
| batch 고정비 절감 (샘플러를 **안전하게** 병렬화) | 35B에서 STEP trace로 sampler 비중 재확인 | 09-04 2B에서 tail 비용의 절반. 폭과 무관하게 이득. **구현은 이미 있으나 기본 비활성이다** — 아래 참조 |
| resident 상향(32 → 64/128/256) | §0.5 3번의 수용·비행 budget을 **먼저 세운 뒤** | **독립 실험 축.** 09-09 `pressure`를 막던 해제 경로의 receipt 예산(제어 batch가 63건까지만 허용)은 P-2 5번에서 해소했고 resident 256 실기 통과를 확인했다. **그러나 수용·비행 budget(§0.5 3번)은 그대로 남아 있다** — 수용 경로에는 아직 pending 개수·바이트·토큰 예산이 없다. 정책 변경과 한 arm에 묶지 않는다. 폭이 resident에 선형이라는 근거도 아직 없다 |
| 1 ms 재시도를 capacity wake로 교체 | 유휴 분포에 재시도 간격이 보이면 | §0.5 2번과 공유 |
| 오프로딩 경로 stage 233 ms 분해 | 동일 template로 다시 잰 뒤 | 별도 판정 |

**샘플러 병렬화의 실제 상태.** `P4_STAGED_SAMPLE_THREADS=N`은 이미 있고, 09-03 2B 8회 교차
실행에서 생성 tok/s 194.06 → 213.45(+10.0%)로 두 분포가 겹치지 않았다. 그런데 **기본값은 1이고,
그래야 한다.** 행마다 sampler는 따로지만 모든 worker가 같은 `ctx_`로 `common_sampler_sample`을
부르고, 상류는 그 안에서 `llama_synchronize()`(`t_eval_us`·`n_eval`·`n_queued_tokens`를 잠금 없이
갱신)와 `get_logits_ith()`(`output_reorder()`가 `logits.data` 행을 제자리에서 교환)를 거친다.
**답을 읽어 가는 버퍼에 대한 데이터 경합이다.** judge 통과는 경합이 없었다는 증거가 아니다.
따라서 이 후보의 내용은 "병렬화한다"가 아니라 **"한 스레드로 한 번 동기화하고 logits를 불변 복사한
뒤 독립 sampler를 돌린다"**이며, 고정 seed·고정 batch 구성·thread sanitizer로 먼저 증명한다.
09-03이 남긴 또 하나의 직렬 비용은 새 sequence마다의 sampler 생성으로, sampler 표를 쓰므로
본질적으로 직렬이다. 별도 대상이다.

**하지 않는 것:** prefill·decode 혼합 복원. 혼합 0/2,459는 결함이 아니라 정책이다. 원자료 HELLO는
`equal_sequence_ubatch=1`이고, `scheduler.rs:280`의 `plan_equal_ordinary`는 decode 한 행이 공통 폭을
1로 만들어 수천 행이 준비된 prompt를 한 행씩 보내던 문제를 막으려고 두 집단을 나눈다. 되돌리면
그 문제가 돌아온다.

**균형화의 크기 추정(약속이 아니다).** 이 실행 자신의 두 집단으로 선형 적합하면 tail은
`34.76 ms + 3.443 ms × 행`이다. 절편이 09-04의 34.2 ms와 거의 같고, 폭 15.80을 넣으면 89.17 ms로
실측 89.68 ms와 맞는다. 비용이 폭에 선형이므로 **batch 수를 유지한 재배분만으로는 이득이 없다.**
이득은 batch 수가 줄 때만 나온다. 38,084행을 폭 32로 모으면 1,190 batch, tail 144.93 ms가 되어
tail만으로는 최대 220.8 decode row/s가 된다(현재 116.87). 이 값은 ① 적합이 폭 32까지 외삽되고
② 두 집단을 실제로 합칠 수 있으며 ③ tail이 유일한 제약이라는 세 가정에 전부 의존한다.
**승격 근거가 아니라 실험을 정당화하는 크기다.** 현재 tail 총합은 벽시계의 66.3%로 아직 포화가 아니다.

#### P-5. 판정 규율

- 비교는 검증 규약 H5를 따른다. 같은 모델·요청·토폴로지·KV 용량·resident·offload layout에서 정책만
  바꾸고, paired 8회 이상, holdout 4쌍, **유효** TPS 중앙 개선 ≥5%, paired 95% CI 하한 양수,
  TTFT p95 ≤1.10배, ITL p95 ≤1.05배와 절대 SLO를 모두 요구한다.
- **GPU 사용률만으로 승격하지 않는다.** 09-04가 보여준 대로 사용률과 겹침은 고정비를 반복해 내는
  동안에도 오른다. 사용률이 올라도 유효 생성 TPS가 내려가면 기각한다.
- 사용률·겹침을 인용할 때는 정의와 분석창을 함께 적는다. `two_or_more_open_pct`(31.6%)는 **stage
  서버 RPC 구간의 겹침**이지 device time이 아니다. tail RPC에는 CPU 샘플링과 복사가 들어 있다.
  한 stage 이상이 열려 있던 비율은 99.3%다. 적재·정리를 포함한 캡처와 stage 실행창도 다르다
  (35B는 각각 16.1/16.7%와 29.3/30.7%).
- 처리량에는 계산식과 품질 판정 여부를 함께 적는다. 의미 검사를 단순 키워드 통과와 구분한다.

## 1. 완료해야 할 제품 목표

**초대형 모델을 여러 물리 컴퓨터에 분산된 노드에서 실행하고, 강한 요청 웨이브를
연속으로 받아 정상적인 프롬프트·응답을 유지하면서 유효 생성 TPS와 GPU 활용을 최대화한다.**

- 한 호스트의 GPU 두 장에 프로세스 네 개를 띄우는 것은 다중 컴퓨터 증명이 아니다.
- 작은 모델과 35B는 개발·교란 분리·회귀 기준선이다. 그것만으로 초대형 모델 목표를 완료할 수 없다.
- 노드 수는 모델 가중치·KV 수용량·합법적 컷·머신/장치 배치의 제약이다. 필요한 노드를 줄여
  성능을 높이는 것은 고정 토폴로지 배치 전략의 개선 증거가 아니다. 같은 장치의 여러 노드도 지원 대상이다.
- 노드를 늘려도 가중치 중복·공유 KV·가장 빡빡한 스테이지 때문에 수용량이 선형 증가한다고 가정하지 않는다.
- “최적”은 선언한 모델·하드웨어·SLO·워크로드·탐색 범위에서 검증한 최선이다. 전역 최적이나 GPU 100%를 약속하지 않는다.
- 유효 생성 처리량과 유용한 GPU 계산을 함께 개선한다. 사용률을 올리기 위해 작은 배치·재계산·polling을 늘려 TPS를 낮추지 않는다.
- **최종 성과 증거는 정상 프롬프트와 응답 전문을 보존한 다중 컴퓨터 실기 강한 웨이브 실행뿐이다.**
  결정론적 시험과 mock은 그 실행에 들어가기 위한 필수 안전성 게이트이지 최종 성과 증명이 아니다.

### 현재 승인된 자원과 실기 확장 순서 (2026-09-07)

사용자가 지정한 모델 후보는 **`S:\models` 전체**, 실기 GPU 예산은 **RTX 3090 두 장**이다.
RAM 오프로딩도 허용하며, **VRAM-only에서 충분한 검증을 마친 뒤 RAM 오프로딩이 필요한 더 큰 모델**로
확장한다. 이 순서는 B6/B7 안의 자원별 검증 순서이지 B1~B5의 안전성 게이트를 건너뛰는 허가가 아니다.

1. 모델 디렉터리 전체를 inventory로 만든다. split GGUF는 한 모델로 묶고 mmproj/LoRA/embedding 등
   보조·비생성 artifact를 구분한다. 각 논리 모델/variant에 감사 상태·필요 자원·선택 또는 제외 이유를 남긴다.
   파일 크기나 이름만으로 지원/실행 가능을 승인하지 않으며, 지원하지 않는 memory family를 성능 시험 때문에 열지 않는다.
2. **VRAM-only 기준선**: 감사된 모델 중 실제 가중치·KV·compute/전송 buffer·상주 동시성을 두 GPU 안에
   수용하는 모델로 검증 규약 H0의 자원 단계 게이트를 통과한다. 작은 모델만으로 큰 모델 배치 적합성을 판정하지 않는다.
3. **RAM 오프로딩 확장**: 앞 게이트를 통과한 뒤 GPU+RAM의 실제 예산 안에서 더 큰 후보를 평가한다.
   CPU 계산, host-resident 가중치/KV, staging/pinned buffer, 전송을 구분하고 placement를 봉인한다.
   각 후보의 load/정상 응답/웨이브/메모리/성능 판정을 따로 기록한다. 정상 거부·자원 부족·미감사는 통과가 아니다.
4. 각 자원 단계의 정책 A/B는 모델·quant·placement·context·resident·워크로드를 고정한다.
   다른 모델의 VRAM-only TPS와 RAM 오프로딩 TPS 차이를 배치 정책 효과로 계산하지 않는다.

읽기 전용 확인에서 두 3090은 **M42-SERVER2 한 물리 호스트**에 있었다. 이 fleet에서의 성과는
단일 호스트/두 GPU 검증이다. 다중 컴퓨터라는 장기 목표와 H6은 별도이며, 다른 호스트를 임의로
추가하거나 RAM 사용을 두 번째 컴퓨터의 증거로 세지 않는다. H6 미충족 때문에 승인된 로컬 안전성·
현재 자원 내 실기 검증까지 중단하지도 않는다. 최종 목표 전체 완료와 현재 자원 내 완료를 구분한다.

`S:\models`는 실행 계정에서 접근을 확인한다. SSH 비대화형 세션에서 S:가 보이지 않는 사실만으로
모델 부재를 선언하거나 경로를 임의 변경하지 않는다. 접속 정보 문서는 저장소 밖에 두고 암호/토큰은
명세·로그에 복사하지 않는다. 모델 선택과 예산은 검증 규약 H0의 명세에 고정한다.

2026-09-07의 현재 로컬 실행 계정에서는 S:를 읽을 수 있었다. GGUF 경로/크기/mtime 예비 목록은
`target/model-file-inventory-20260907-01.json`에 보존했다(156파일, 파일명으로 묶은63그룹).
파일명 기반 임시 분류는 모델 후보40/embedding1/projector22이며 shard 번호 누락은 없었다.
이는 GGUF 헤더·전체 content hash·family 감사·메모리 계획·원격 계정 접근·load 성공의 증명이 아니고,
non-GGUF 전체 목록도 아니다. H0의 논리 모델/variant inventory를 완료했다고 읽지 않는다.

## 2. 새 세션의 첫 30분

1. 저장소 루트에서 `git status --short --branch`, `git log -5 --oneline`으로 기준을 확인한다.
   기준 커밋 이후 변경은 §4의 소스 경로에서 감사하고, 다른 사람이 남긴 dirty 변경은 보존한다.
2. 이 문서와 검증 규약·격리 계약을 읽는다. 모든 역사 문서를 처음부터 읽고 과거 순서를 복원하지 않는다.
3. `entrypoints/agent/src/main.rs::main`을 확인한다. 기본은 `event_runtime`이고
   `P4_AGENT_SERVICE_RUNTIME`은 과거 Chain/Hop 경로다. 후자의 fairness/queue 시험을 현재 경로 증명으로 세지 않는다.
4. §3의 기준 커밋 감사와 마지막 진행 기록을 대조한다. 이미 고정한 반례를 다시 발견했다고 하지 말고,
   현재 남은 반례를 실제 worker/native 소비 경로에서 먼저 고정한다.
5. §0의 중단/검증 예산을 먼저 확인한다. 재개 승인 후에만 고정된 검증을 실행하고 실제 실행/제외/실패를 기록한다.
6. 첫 미통과 단계 B0의 증거를 확인한 뒤 §0의 B1/B2 잔여부터 계속한다.
   후속 구현을 무시하고 수정 전 테스트로 회귀시키거나, B1 전체가 끝났다고 전제하지 않는다.

모델 후보 경로와 현재 GPU 범위는 §1의 사용자 지정을 따른다. 아직 확정하지 못한 입력은
실행 계정의 파일 접근, 후보별 전체 artifact identity/메모리 계획, 추가 다중 호스트 자원이다.
기존 하네스의 경로·IP·계정은 예시가 아니라 과거 구성값이다. 사용 가능성과 권한을 다시 확인한다.
입력이 없다고 작은 모델이나 한 호스트를 최종 대상으로 자동 대체하지 않는다.

## 3. 감사된 현재 상태

아래 `확인`은 기준 커밋에 한정한다. 과거 GPU 수치는 이번 HEAD의 재측정이 아니다.

| 영역 | 판정 | 근거와 남은 일 |
| --- | --- | --- |
| 현재 실행 경로 | 확인 | `main.rs::main` → `event_runtime`; 실제 event worker 사용 |
| 배치 선택기 | 부분 구현 | `scheduler.rs::plan_equal_ordinary`: cohort별 sequence ID 회전, patience; `plan_ordinary`: 일반 행 배분. 전체 분산 스케줄러가 아님 |
| 요청별 기아 반례 | 해당 반례 해소 | 17 prefill·decode 1·용량 8·900회 계획에서 모두 진행. 최대 간격 끝 구간 포함; 실시간 TTFT 보장은 아님 |
| 정산 공유 | 일부 확인 | `node/state.rs::RequestState::settle_fragment`를 worker와 simulator가 호출. outstanding·prompt cursor·ready 일부만 공유 |
| 정산 전체 원자성 | **미해결 R-A** | `worker/release.rs::Worker::tail`: 검증 전 open execution 제거, 여러 요청 중 일부 먼저 변경. 오류 발행 후 worker가 계속될 수 있음 |
| 출력 효과의 승인 경계 | **미해결 R-D** | `worker/drive.rs::Worker::emit_tail_results`: 꼬리가 OUTER 출력 후 head 반환을 발행. head 원자화만으로 malformed 반환의 외부 부작용 0을 증명할 수 없음 |
| 발행-반환 동일성/멱등 | **미해결 R-B** | 이전 execution 반환이 다음 fragment를 소비; outcome 없는 부분 prefill은 다른 sequence와 위치 구간도 수용 가능 |
| 공유 경로 회귀 시험 | **미해결 R-C** | `simulator_tests.rs::the_worker_and_this_model_settle_through_one_transition`은 직접 함수 시험. 실제 Simulation 호출을 별도 부기로 바꿔도 11개 통과한 검수 반례 |
| simulator | 고정 지연 완료 모델 | 실제 selector 사용. admission·RPC·분할·credit·release/shutdown을 모두 통과하지 않음 |
| worker 시험 | tail 지점만 | 인코딩된 CapsuleSet의 정상/과다행/미발행 거부 3건. 가짜 stage와 전체 worker loop는 아직 없음 |
| 열린 배치 원장 | 일부 | 논리 배치별 execution 집합 존재. 요청 정산과 한 transaction으로 결속되지 않음 |
| backpressure | 일부 | 양방향 보존/Full·Closed 구분 존재. capacity notification·완전 drain·취소 검증은 남음 |
| admission/credit | 미완 | 슬롯 외 KV 셀 예약, bounded pending, 다중 노드 예약과 edge row/byte credit 완결 필요 |
| native 호환 경계 | 부분 | `src/` 격리, pin/patch 큐 존재. `common/` 잔여, 제품 identity 강제·실제 placement 결속·backend conformance 남음 |
| 영속 KV/스냅샷 | 기반 구현 + 목표 계약 | 파일/영수증/코디네이터가 존재. 새 namespace·정체성·수렴·스냅샷 계약이 전부 구현된 것은 아님 |
| 실기 하네스 | 개발 기반 | `test/benchmarks/p4-4node/` 추적. 현 remote 구성은 원격 한 호스트 안에 stage들을 배치; 임의 다중 호스트 최종 runner는 보강 필요 |
| 최종 초대형 모델 웨이브 | **미증명** | 정상 응답·다중 호스트·봉인된 반복 비교를 동시에 만족하는 완료 증거 없음 |

위 표는 기준 커밋의 초기 감사다. 이후 수정 전 실패 반례를 RED로 봉인하고, 다음 구현 slice에서
발행 권위·반환/효과 전이를 수정했다. **초기 표 또는 RED 집계를 현재 상태로 재사용하지 않는다.**
반례별 실제 도달 범위와 최신 전체 집계·명령은
[2026-09-06 감사 기록](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)에 보존한다.
C++/실기 승격과 CPU-only Rust 검증은 별개다. 외부 fixture 기능은 기본 집계에 포함되지 않는다.

### 받아들이지 않는 과거 결론

- “4스테이지보다 2스테이지가 빠르므로 GPU 수까지만 노드를 만든다”: 특정 배치 실험의 일반화다. 용량 요구를 무시한 제품 규칙으로 사용 금지.
- “RPC가 겹치므로 여러 GPU가 동시에 계산했다”: host service span과 device kernel span은 다르다.
- “빈 꼬리의 홉이 2 ms이므로 전송은 항상 싸다”: 특정 모델·한 호스트/링크 관측이다. 초대형 모델·다중 머신에 일반화 금지.
- “30행과 1,232행의 TTFT가 비슷하므로 admission이 원인이다”: 도착·대기·tokenize·prefill·첫 출력 시점을 분해하기 전 인과 확정 금지.
- “혼합 배치 0개이면 깊이 1이다”, “출력에 U+FFFD가 없으면 의미가 정상이다”: 둘 다 충분조건이 아니다.
- “단위 시험이 초록이면 현재 실기 품질도 초록이다”: 코드 안전성, 수치 회귀, 자연어 품질, 성능의 게이트는 별개다.

## 4. 책임 경계와 코드 지도

| 책임 | 현재 진입점/소유 | 목표 |
| --- | --- | --- |
| 요청·도착 웨이브·SLO·토폴로지·스냅샷 트리거 | `tools/event-drive/`, `test/benchmarks/p4-4node/` (OUTER) | 제품 요구와 실제 placement 명세 |
| 전달·순서·mailbox·노드 lifecycle | `layers/agent/`, `layers/adapters/adapter/`, `entrypoints/agent/src/event_runtime/mod.rs` | backend 중립 보존, 수용 통지·drain |
| runnable/배치 선택 | `layers/adapters/llamacpp/staged/adapter/src/v2/scheduler.rs` | 순수·결정론적 정책; 독립 상태 전이와 결합 |
| 요청·비행·예약 원장 | `v2/node/state.rs`, `v2/node/worker/drive.rs`, `release.rs`, `settlement.rs` | 발행 기록과 원자적·멱등 정산의 단일 소유 |
| 분할·캡슐 | `v2/capsule.rs`, `v2/capsule/`, `v2/logical.rs` | 행 소유와 실제 physical membership 대조 |
| 완료 모델 | `v2/simulator.rs` | 가짜 시간/engine만 대체; 상태 변경 로직 복제 금지 |
| native 실행 | `staged/server/src/runtime/`, `staged/server/src/compat/`, `server/CMakeLists.txt` | public 타입/facade 경계, upstream 편의 기능은 opaque plan으로 보존 |
| llama.cpp와 구상 backend | `layers/adapters/llamacpp/upstream/`, `staged/compat/` | pin별 prepare·CPU·선언 production backend 감사 |

위 `v2/`의 기준 루트는 `layers/adapters/llamacpp/staged/adapter/src/v2/`다.
정책/원장 → P4 소유 capability → native compat → llama.cpp 추상층 → ggml/backend(CUDA·CPU·Metal 등)의 경계를 지킨다.
P4 코어에 모델별 batch·KV 규칙을 넣지 않는다. `common_params`를 필드 몇 개로 복제해 upstream 간접 옵션을 잃지 않는다.
신규 크레이트 분리는 공유가 실제 성립한 뒤 선택하는 포장 변경이지 B1의 목표가 아니다.
각 층의 변경 권한·public 타입·CMake/Rust 의존 허용 범위·pin 갱신 과정은 [격리 계약](layer-isolation-contract.md)을 따른다.
특히 adapter-owned 의미 DTO를 generic P4 protocol로 올리지 않는다. 격리 계약의 인터페이스 대장과
의존 manifest를 구현 산출물로 만들고, L1의 상태 확정과 L5의 native/출력 효과 실행을 분리한다.

## 5. 목표 상태 전이 — 구현해야 할 계약

현재 함수 시그니처가 아래 계약을 이미 구현했다고 해석하지 않는다.

### 발행과 정산

- `plan`: 상태 snapshot과 budget을 읽고 후보를 만든다. 후보 생성만으로 발행량·credit·KV를 소비하지 않는다.
- `reserve`: 참여 스테이지 KV/shape와 edge budget을 전부 확보한다. 부분 실패는 안전하게 해제/수렴한다.
- `issue`: 실제 받아들여진 발행을 fragment 기록에 결속한다. 전송 성공이 불확실하면 미발행으로 돌리지 말고 `Uncertain`으로 reconciliation한다.
- `validate_return`: generation/session/sequence/fragment/execution/phase/position range/row membership/outcome을 **발행 기록과** 대조한다.
- `commit_settlement`: 반환 이벤트가 건드리는 요청·비행 원장·예약·credit·후속 출력 의도를 함께 반영한다.
  기본 계약은 이벤트 전체 사전 검증 후 원자 반영이다. 부분 수용으로 바꾸려면 별도 receipt와 재시도 계약이 먼저다.
- `drain/release`: transport ACK, compute 완료, KV 정지점, sequence release attest를 별도 사건으로 취급한다.

fragment 기록에는 generation, session/sequence 세대, 논리 dispatch·fragment ID, physical execution 집합,
phase, `[start,end)`, membership digest, 예약/credit ticket, 상태와 정산 증거가 필요하다.
정확한 wire encoding은 구현 slice에서 버전화하고 기존 content-type 소유를 지킨다. raw counter를 식별자의 대용으로 쓰지 않는다.
같은 ID·같은 완료는 no-op/기존 receipt 재응답, 같은 ID·다른 내용은 conflict 거부한다.
미등록 ID, 이전 세대, 다른 시퀀스·구간·phase, 중복 행은 상태 변경 전에 거부한다.
발행되지 않은 위치로 진행하거나, 이전 반환이 다음 비행을 소비하거나, 정산 전 sequence를 재사용하면 실패다.

엔진/가짜 엔진은 모두 정규화된 결과를 반환한다. token 값을 얻는 과정만 다르고 generated count,
다음 입력 위치, stop/length, verify/replay continuation을 적용하는 의미론은 공유한다.
출력 publisher가 Full이면 완료 의도를 보존하며, 재시도는 같은 논리 결과를 두 번 발행하지 않는다.

### 배치 계획의 계약

- KV 상주 용량, resident sequence 한도, 이번 decode 폭, 논리 batch 한도, physical ubatch 한도,
  edge credit, 장치별 실행 슬롯은 서로 다른 축이다. 하나의 `parallel` 또는 bool로 합치지 않는다.
- 일반 attention은 합법적 decode와 chunked prefill을 row budget 안에서 배분한다.
- equal-width 계열은 cohort를 전체 eligible 수요에서 선택하고, cohort별 회전으로 **개별 요청** 진행을 보장한다.
- decode 다음 토큰은 선행 결과 없이 발행하지 않는다. prefill 다중 fragment는 KV 순서·행/byte credit을 증명한 뒤 허용한다.
- verify/replay와 speculative 창의 원자성은 throughput 때문에 완화하지 않는다.
- ready인데 미발행한 행은 `shape/credit/KV/cohort/in_flight/deadline` 등 이유로 분류한다.
  “남긴 행 0”은 목표 자체가 아니다. 의존성 때문에 보낼 수 없는 행을 runnable로 부풀리지 않는다.
- 공정성은 cohort 횟수뿐 아니라 요청별 첫 선택·중간 간격·마지막 대기 구간과 실시간 queue age로 확인한다.

## 6. 실행 단계와 승격 조건

상태 표기: `TODO`, `IN_PROGRESS`, `PASS`, `BLOCKED`. 필요한 증거가 없으면 PASS가 아니다.
각 단계는 [검증 규약](distributed-batching-verification.md)의 시험 ID와 연결한다.
B0의 반례 봉인은 **수정 전 실패를 확인·보존**하는 작업이다. 이를 통과 구현으로 보고하지 않는다.
해당 반례가 정상/부정 경로 및 mutation까지 통과하는 것은 B1의 종료 조건이다.
기존 suite가 통과한다는 이유로 반례를 생략하거나, 예상 실패를 숨겨 B0/B1을 동시에 PASS로 만들지 않는다.

| 단계 | 현재 | 산출물 / 종료 조건 |
| --- | --- | --- |
| B0 기준과 반례 봉인 | IN_PROGRESS | 기준 HEAD/실행 경로 확인, R-A/B/C 정식 failing tests, 문서·시험 inventory, target 후보/자원/권한 목록. T00~T04 |
| B1 발행 원장 + 원자적 정산 | IN_PROGRESS | 헤드 권위·후보 정산·효과 의도, 후속 incarnation/제어 receipt·승인 후 fairness commit 구현. 물리 실행 전 홉 멱등, 정상화 전이 전체 공유와 실패 수렴이 남음. T10~T19 |
| B2 실제 event worker 통합 | IN_PROGRESS | ordinary actual run 2/4/8-stage, speculative 2/4-stage·busy UNLOAD·head OUTPUT/발행 증거·소유 관측의 현재-run 소비 회귀와 별도 broker 포화 검증. 취소·drain·capacity notification·통합 순환망·재시작 freshness는 남음. T20~T28 |
| B3 수용·KV 예약·edge credit | TODO | bounded pending와 byte/token 예산, deadline·명시적 거절, 다중 노드 all-or-none 예약, row/byte credit, leak/over-admit 0. T30~T38 |
| B4 continuous batching 정책 | TODO | 전체 runnable 재선택, 일반/등폭/atomic 전략, 요청별 fairness, batch/ubatch 분리, 공유 전이를 쓰는 simulator/reference와 worker 대조. T40~T47 |
| B5 실행 신원·native·다중 호스트 하네스 | IN_PROGRESS | B1에 필요한 versioned 실행 identity와 제품 LOAD bind·native guard부터 보강. 실제 layout/model/build/ABI 결속·full/relink 격리·선언 backend·다중 호스트 runner는 남음. I00~I09/T50~T58 |
| B6 초대형 모델 웨이브 기준선 | TODO | §1의 자원 단계와 H0~H4: 정상 요청/응답 전문, 강한 겹치는 웨이브, 유효 TPS/GPU 요약, 재시작 없이 반복. 현재 fleet 성과와 2개 이상 물리 컴퓨터 최종 승격을 별도 기록 |
| B7 배치 최적화와 반증 | TODO | 토폴로지 고정 paired A/B + holdout, 단계별 cost 분해·credit-aware issue·prefill chunk/폭 선택; H5/H6. 승인된 유효 TPS/GPU Pareto 후보 |
| B8 지속 운영·최종 인수인계 | TODO | H7 soak/fault, 선언 backend/upstream 회귀, 재현 가능한 증거 bundle, 모든 필수 gate PASS와 남은 비필수 범위 공개 |

승격 의존 관계: B0 후 B1, B1 후 B2, B1/B2 후 B3, B2/B3 후 B4.
B1의 실제 소비 경로 검증을 위해 B2의 최소 fake-stage 연결을 먼저 작성할 수 있다. 이는 B2 승격이나
실기 최적화 선행의 허가가 아니며, 양쪽의 남은 시험을 생략하지 않는다.
B5의 환경 발견·신원 설계는 B1과 병행 가능하나 실기 승격은 B1~B5의 관련 gate를 모두 요구한다.
B6 후 B7, B7 후 B8이다. 작은 native smoke는 기존 감사 조합의 회귀 진단용으로 허용되지만 B6를 대체하지 않는다.
GPU 실험을 기다리며 같은 arm의 checkout을 수정하지 않는다.
계층 격리는 B5만의 마지막 청소가 아니다. B1~B4의 매 변경부터 I00/I03의 pure 경계를 지키고,
B5에서 I 전체를 완성하며, B8 및 이후 모든 채택 pin에서 반복한다.

### B1의 첫 작업을 구체적으로 고정

1. 열린 배치를 등록한 과다 반환, A 정상+B 잘못된 혼합 반환, 새 이벤트에 담긴 옛 execution,
   outcome 없는 wrong sequence/range를 `Worker::tail`/`handle`로 재현한다.
2. `close_execution`을 단순히 뒤로 옮기는 것으로 끝내지 않는다. 요청 전체 검증과 outcome 오류·출력 효과까지 사전 검증한다.
   꼬리 선출력을 제거하거나 승인 receipt로 차단하여, head가 거부한 반환의 출력이 먼저 나가지 않게 한다.
3. 발행 원장의 expected range/membership와 반환을 대조하는 validated settlement를 만들고 한 번만 적용한다.
4. Simulation의 실제 도착 경로에도 동일 malformed fragment를 주입한다. 직접 RequestState 함수 시험만으로 대체하지 않는다.
5. 새 원장과 기존 원장을 병행할 때 shadow mismatch는 숨기지 않는다. 두 곳이 독립적으로 상태를 갱신하는 전환기는 금지한다.

### B5 격리 구현의 종료 산출물

격리 계약의 경계별 대장을 코드 심볼/target과 연결한다. 기존 큰 `p4_llama_compat.cpp` 파일을 키우는 것 자체는 목표가 아니다.

1. 현재 dependency/include/link/type/codec 표면을 inventory로 봉인하고 I00~I02의 정상·침범 fixture를 먼저 만든다.
2. common 파싱·옵션·grammar 연산과 내부 단언을 compat 구현/전용 시험 타깃으로 옮긴다.
   문법을 재발명하거나 필드 getter 복제로 white-box 시험을 약화하지 않는다.
3. public/internal header 경로, `Impl` 접근, direct/transitive link를 정리한다. full build와 imported relink 모두 검증한다.
4. native shell와 engine bridge의 실제 허용 API·수명·실패 상태를 확정하고, capsule codec의 안정 코드표 또는
   opaque codec 협상을 구현한다. 기존 raw 정수 필드를 자동으로 중립 ABI라 부르지 않는다.
5. engine/common 변화와 ggml/backend 변화 각각에 합성 반증·실제 채택 pin 회귀를 연결한다.
   제품 LOAD identity 강제와 선언 backend conformance를 통과한 뒤만 실기 승격한다.

각 slice는 격리 계약의 최소 manifest 레코드와 검증 규약의 I 하위 사례를 함께 납품한다.
특히 Rust/native의 실행 권한 이중 구현 대조, 실제 적재 라이브러리·plugin 신원, opaque handle 수명,
model-free/model-required 시험 분리는 뒤의 성능 수치로 면제할 수 없다. 허용 module의 변경으로
흡수한 것과 상위 계약 변경이 필요한 것을 구분해 보고하며, getter나 새 폴더 개수를 성과로 세지 않는다.

이 단계는 B1의 안전성 수정과 별개로 병행 준비할 수 있다. B1~B4에서도 새 상태 권한 누출이나
native 의존을 추가하면 해당 slice를 승인하지 않는다. 자세한 책임/허용 의존은 격리 계약을 단독 소유로 유지한다.

### B7에서 비교할 정책과 금지할 접근

먼저 row 폭·prefill chunk·decode 예약 비중·age bound·edge issue budget을 작고 사전 선언한 후보군으로 비교한다.
호스트/장치별 queue, tokenize, compute, sample, encode/copy, network, tail wait를 나눈 cost model을 사용한다.
cohort를 합치려고 pipeline 전체가 비기를 기다리거나, sampler 안전성 없이 병렬화하거나,
로컬 실측 한 번으로 전송/노드 수를 원인으로 단정하지 않는다.
컷/placement를 변경하는 연구는 별도 arm이며 KV 수용량·모델·품질·연산량 차이를 함께 보고한다.
1GPU/replica/stock llama-server는 맞는 조건에서 진단 대조군이다. 분산 용량 목표의 대체 제품은 아니다.

### B4 정책 구현의 구체적인 출발점

고정한 upstream의 `tools/server/server-context.cpp::update_slots`, `can_batch_with`, prompt 추가/분할 경로를
직접 읽어 재사용할 의미론과 분산 때문에 달라지는 계약의 대응표를 먼저 작성한다. HTTP/slot/server_context
구현을 가져오거나 llama.cpp가 이미 분산 정산·credit을 보장한다고 가정하지 않는다.

1. 호출자 상태를 `resident/eligible/blocked`로 정규화한다. token 의존성, cache 명령 정지점,
   model/LoRA/shape 호환성은 단순 ready 개수와 별개다.
2. 합법적 shape와 최대 row는 **참여 stage 전체 capability의 교집합**에서 구한다. 헤드에서만 맞는 batch는 거부한다.
3. 일반 attention은 decode와 chunked prefill을 함께 검토한다. equal-width·verify/replay는 그 제약을 명시하는 별도 전략을 쓴다.
4. deterministic tie-break, 요청별 회전·age bound, row/byte/KV budget을 명시한다.
   계획 결과와 발행 확정은 분리하고 issue 거부/부분 성공/Uncertain 경로가 cursor와 fairness를 잘못 소비하지 않게 한다.
5. 작은 상태 공간의 독립 reference allocator와 전수 대조한다. oracle은 한 시점의 명시 목적/제약에 대한 기준이지
   장래 GPU 비용을 모르는 전역 최적 oracle이 아니다. 실제 비용의 후보 선택은 B7에서 한다.
6. capability/실측 cost profile을 입력 데이터로 버전화한다. 환경변수 임계값을 늘려 의미론의 빈칸을 메우지 않는다.

현재 실험 손잡이(min rows/open batches/issue rows/prefill fragments)는 검증된 새 정책에 연결되기 전까지
기본 비활성 또는 기존 단일 fragment 동작을 유지한다. 남길 손잡이는 적용 경로·예산·안전성·성능 반증을 모두 갖춰야 한다.

## 7. 기존 U/P 시리즈와의 연결 — 버리지 않되 순서는 교체

구체적인 저장 계약·결함 배경은 [구 계획](adapter-restructure-plan.md)에 남긴다. 오래된 완료/미착수 표현은 당시 상태다.

| 구 항목 | 새 소유/처리 |
| --- | --- |
| U0 | B5 + B8 per-pin 회귀. 모든 compat 작업이 B1의 pure correctness 작업을 막지는 않음 |
| P-1 | B0/B5 identity·재현성, 아래 K 저장 분기의 record identity |
| P0 | K 분기: namespace·receipt·bundle·CONTROL |
| P1a/P1b | B1/B2/B3: 실행 원장·동적 셀·슬롯 재사용 반례. 관찰만으로 수정 완료 금지 |
| P2 | K 분기 fault/복원 행렬. 사용하지 않는 영속 기능을 batch correctness 선행 조건으로 묶지 않음 |
| P2.5 | B7의 정적 batch/ubatch 보정. 저장 정체성 변화는 K 게이트 적용 |
| P3 | B3 수용/셀 예약과 K 스냅샷 정책 이행으로 분리 |
| P4 | B1/B4 기전-정책 분리. 크레이트 생성과 골든 일치만으로 완료 금지 |
| P4.5 | B3 credit; 정산 동일성과 원자성은 B1부터 필요 |
| P5 | B2/B4/B6/B7. 깊이 존재 여부가 아니라 안전하고 유효한 overlap/서비스 증명 |
| P6 | B5/B7: 실제 다중 머신 전송·고정비 프로파일 후 최적화. 병목을 미리 확정하지 않음 |
| P7 | B5/B8 backend/model 승격과 K의 청크 persist. 선언 외 조합은 계속 거부 |

### K: 영속·스냅샷/확장 분기

K0 namespace/CONTROL/불변 bundle → K1 Persist/Restore/Discard 장애 수렴 → K2 Checkpoint/Fork/RestoreInto/List,
LCP/TrimTo와 stage별 정지점 → K3 큰 상태 청크/교차 backend·ABI 행렬 순으로 다룬다.
세부 계약은 [저장 규약](kv-state-store-convention.md)이 소유한다. 기존 Committing·epoch·read-pin·quota 등 열린 결정을 버리지 않는다.
자동 TTL 축출·prefix 재사용·KV 복원을 켜면 관련 K 게이트가 필수다. B6 웨이브가 resident-only 예산에 들어가면
해당 기능을 꺼 둔 채 핵심 분산 배치 목표를 먼저 검증할 수 있다. 최종 보고는 꺼 둔 기능을 완료로 세지 않는다.
미감사 모델을 최종 대상으로 선택했다면 해당 memory/backend 감사는 선택 사항이 아니라 B5의 차단 게이트다.

### 기존 결함·열린 결정의 누락 방지 색인

아래는 **소유권 이관**이지 결함이 현재 재현되거나 해소됐다는 판정이 아니다.
기준 커밋 이후 다시 확인하고, 닫힌 과거 결함도 회귀 시험을 유지한다.
세부 배경/원문은 구 계획과 분야 규약, 실행 판정은 검증 규약의 T/K/H ID를 따른다.

| 구 ID | 새 소유 / 확인할 게이트 |
| --- | --- |
| D1 | B1~B4/B6: 실제 issue/settle/credit/overlap; T14/T20/T21/T38/T44, H2/H4. 과거 깊이 1 주장을 현재로 복사하지 않음 |
| D2/D3/D6/D13~D18 | K0~K2: namespace·identity·영수증·번들·세션 직렬화·장애 수렴; K00~K07 |
| D4 | B1/B2: slot/sequence 재사용 및 전달 손실을 분리 감사; T12/T18/T24/T25/T58. O13의 수리로 모든 position 결함을 닫지 않음 |
| D5/D12 | B3/B5: KV 예약/점유와 wire telemetry; T31/T32/T37/T57 |
| D7 | B5/B7: 실제 cut-set/copy/network 프로파일; H4~H6. 다중 호스트에서 비용 재측정 |
| D8/D9 | B3/B5/B7: compute/SWA 실제 메모리 회계와 batch/ubatch; T31/T37/T44/T53, H5 |
| D10/D11 | K2/K3: 큰 상태/프리픽스 재사용; K07/K09 |
| D19~D22 | B5/B8: pin·패치·include·실행 identity·EOL; T50~T53 |
| O1 | B3/B5: reserved/used/last-access의 telemetry 버전·시점·소유; T31/T57 |
| O2 | B5: stage ABI 실제 함수/타입·upstream 의미 변경 감사; T50/T52/T53 |
| O3 | B5/K1: 계열별 소형 모델/골든 state 자산의 실제 존재·재생; T53/K03/K04 |
| O4 | B3/K0: 예약/lease 전순서, TTL·Commit 경합·Prepared 회계; T32/K01/K09 |
| O5/O13 | 기존 해소 보고의 회귀 유지: session key 전달과 cancel-safe wire; T00/T24/T54/T58 |
| O6/O8 | K2: 정확한 snapshot 동사/ID 계약과 OUTER 원장 복구; K06 |
| O7/O11 | B3/K3: resident/disk/RAM capability·예산·ENOSPC; T31/K09 |
| O9 | B1/B2/K2: transport credit와 quiescence 분리; T25/T35/K05 |
| O10 | K2: read-pin/storage domain/Discard; K08 |
| O12 | B2/B6/B8: 동일 agent의 반복 수용과 연결 수명; T28, H2/H7 |

새 결함은 재현 경로·소유 단계·실행할 시험 ID를 함께 등록한다. “이미 등재됨”은 발견 이력의 분류일 뿐,
현재 차단 결함을 무시하거나 승격해도 된다는 뜻이 아니다.

## 8. 상태 기록과 중단 규칙

단계별 기록은 아래 형식을 이 파일 마지막에 누적한다. 증거 수치/응답 전문은 evidence 파일의 링크로만 참조한다.

```text
단계 / 상태 / 날짜:
검증한 source commit + dirty 여부:
계약 변경 및 소유 문서:
통과한 test IDs / 실행 명령 / exit code / 결과 파일:
수정 제거·오류 주입 시 실패한 ID:
실기 run IDs / binary+model+workload digests / machine identities:
미실행·제외·실패·BLOCKED와 이유:
다음 세션이 가장 먼저 실행할 반례/작업:
```

counterexample 미보존, malformed 반환 수용, credit/RSS 초과, 정상 응답 실패, 기록 손상,
arm 정체성 불일치는 해당 승격을 즉시 중단한다. 범위를 줄이거나 실패를 제외해서 통과시키지 않는다.
필수 시험이 없는 단계는 미완이며 “테스트 작성 예정”을 PASS로 기록하지 않는다.

### 2026-09-06 초기 인수인계 기록 — 아래 후속 기록 이전 상태

- B0 IN_PROGRESS: 코드 감사·문서 역할 재정리 및 R-A/B/C의 실패 반례 추가. actual handle 후속 지속,
  뒤 permutation/identity table의 아직 미도달 입력과 전체 worker/effect 경로는 추가 검증 필요.
- B1~B8 TODO. 위 표의 기존 부분 구현을 재사용하되, 해당 단계의 완료 시험을 생략하지 않는다.
- 계층 격리 보강: 권한/API/의존 대장과 upstream 변경별 실패 계약을 명시했으나 I gate 전체 구현은 아님.
  common 타입/Impl·간접 link·codec 격리와 R-D 출력 승인 문제가 남아 있다.
- K 분기는 목표 계약/기반 구현 상태이며 승격 전 재감사 필요.
- 다음 첫 행동: 보존한 T10~T13/T17 실패를 재실행하고, 실제 발행 expected membership 등록 및
  whole-event 검증→원장/효과 의도 commit을 연결한다. 부분 수리로 suite를 덮지 말고 handle·출력 경로까지 확장한다.

### 2026-09-06 후속 구현 — B1/B2 IN_PROGRESS

- 소스: HEAD `a9e1967fc` + 미커밋 어댑터/시험/문서 변경. commit/push/native 빌드/배포/GPU 실행 없음.
- `node/flight.rs::FlightLedger`: issued invocation/owners, 논리 fragment별 physical 집합,
  부분·역순 반환 buffering, bounded terminal receipts, 기존 ID 재사용 거부. **헤드의 원장**이지 전 홉 exactly-once가 아니다.
- `node/state.rs::RequestState::issue_fragment` 및 `settle_fragment`: worker와 Simulation의 실제 발행/도착에서 공유.
  Prepared/AwaitingNative/Uncertain을 구분하고 응답 유실을 미발행으로 되돌리지 않는다.
- `worker/release.rs::Worker::tail`와 `worker/outcome.rs`: 전체 후보 검증→request/flight/후속 intent commit.
  tail의 OUTER 선출력을 제거하고 head 승인 뒤 공개. pending KV ack와 물리 outstanding을 분리했다.
- `worker/effects.rs`: 승인된 output/forward/native 의도를 실행. Full은 기존 대기 경로로 보존,
  Closed/결과 불명은 남은 의도를 보존하고 fence. **메모리 내 보존**이며 재시작 내구 수렴을 구현한 것은 아니다.
- `process/core.rs::ServerControl`을 Box로 주입한다. fake는 native Frame만 만들고 selector/원장/정산을 흉내 내지 않는다.
  실제 handle/drive·native 호출 수·split·마지막 반환 장벽·사후 오류 fence를 검사한다.
  LOAD parser, `Worker::run` 수신 큐, 실제 N-stage broker/transport는 이 최소 seam의 범위 밖이다.
- 오류 반례·변이·최종 전체 집계는 [후속 증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)의 최신 절만 사용한다.
  원래의 R-A~D 반례가 보강됐다고 T10~T28 전체를 PASS로 만들지 않는다.

**다음 세션의 첫 행동과 남은 순서**:

1. 현재 소스/시험을 확인하고 **동일 load·session·request ID·slot 재사용 후 옛 RELEASE/RELEASED/SETTLE 도착** 반례를
   실제 fake 중간·꼬리 worker에 고정한다. `pending_releases[key]=slot`만으로 이전 요청과 새 요청을 구별하지 못한다.
   요청 incarnation과 연산/receipt 식별을 physical issue부터 모든 홉의 native 효과·ack까지 결속한다.
   head nonce만 추가하거나 동일 request ID 재사용 금지로 우회하지 않는다. adapter wire 버전과 native conformance가 필요하다.
2. 중간 노드의 동일 PHYSICAL 재전달이 native KV/sampler를 두 번 만지는 반례를 고정한다.
   edge 수신 원장의 accepted/running/completed/uncertain 및 재연결/이전 세대 처리를 구현한다.
   head의 중복 terminal no-op로 downstream 중복 계산이 해결됐다고 하지 않는다.
3. 실제 `Worker::run`을 N-stage 가짜 네트워크에 연결한다. 입력 큐가 계속 차도 발행 기회를 주는 bounded servicing,
   capacity wake, cancel/reload/drain을 검증한다. 지금의 `try_recv` 무제한 drain은 selector fairness와 별개다.
4. 공유를 작은 counter 함수 둘에서 끝내지 않는다. engine 결과 생성만 대체한 reference가 동일 발행/정산 의미론과
   outcome/stop/position 전이를 사용하도록 확장한다. 구상 native 타입은 순수 원장/정책에 넣지 않는다.
   `Scheduler::plan`이 먼저 바꾸는 cohort resume/decode_runs도 후보 fairness delta로 분리한다.
   실제 drive의 발행 거부→재계획에서 서비스 순번이 소비되지 않는 반례를 먼저 고정한다.
5. 고성능 기준선 전에 불변 요청 입력/라우팅과 작은 진행 후보를 분리한다. 현재 `RequestState::clone`은 긴 prompt와
   원본 Event를 매 발행·반환에 복사하며, effect clone은 cut-set bytes를 재복사할 수 있다.
   발행 등록 시 member index를 만들고 touched request/후속 fragment만 갱신한다. 전체 대조는 독립 시험으로 유지한다.
   CPU 시간·할당/복사 byte가 prompt 길이 또는 무관한 열린 배치에 비례해 증가하지 않는지 검사한다.
6. 그 뒤 B3/B4의 KV/row/byte credit·continuous batching 정책과 B5의 native/제품 신원 격리를 완결하고,
   B6~B8의 **초대형 모델·여러 물리 컴퓨터·강한 웨이브·정상 응답**을 실행한다. 로컬 green은 이 목표를 대신하지 않는다.

B0의 최종 모델/호스트/권한/예산 입력과 B5~B8 실기 증거는 여전히 미확정이다.
현재 반환 receipt는 제한된 메모리 window에서만 동일 결과를 no-op 처리하며, 만료된 ID는 fail-closed다.
active flight/queue/단계간 tensor 전체 메모리 상한은 B3의 별도 gate이고, receipt 상한으로 증명되지 않는다.

### 2026-09-06 후속 기록 2 — 실행 소유권과 정책 후보, B1/B2/B5 IN_PROGRESS

- 동일 load/session/request key/slot 재사용 반례를 actual head/middle/tail 소비 경로에 고정했다.
  늦은 제어/ack가 새 incarnation의 KV·슬롯·pending barrier를 소비하지 않도록 adapter와 native에 결속했다.
  wire·BindLoad·수명/범위의 단독 계약은 [배치 계약](adapter-batching-layers.md)의 실행 소유권 절이다.
- 중간/꼬리의 정확한 SETTLE/RELEASE 재전달은 native 추가 실행 없이 같은 receipt를 반환한다.
  같은 ID의 다른 body/kind, 이전 operation은 효과 없이 거부한다. 여러 control의 합계 예산도
  첫 native 효과 전에 검사하며, 뒤 receipt 축소를 앞의 여유로 미리 계산하지 않는다.
- `Scheduler::prepare_plan_with_physical_capacity`/`commit_plan`을 actual drive와 Simulation에 연결했다.
  후보 거부가 회전·cohort 순번을 바꾸지 않고 승인한 발행만 전진한다. 이 보강은 정책 최적화 결과가 아니다.
- native 소유권/UTF-8 helper는 llama/ggml 타입 없이 빌드한다. 제품 LOAD는 실행 identity를 bind하지만
  모든 build/model/state ABI/actual placement를 강제하는 완성된 B5 경계는 아니다.
- 실제 consumer·RED/GREEN·변이·동결 소스/전체 집계는 [최신 증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)에 있다.
  이전 진행 기록의 미구현 표현은 그 시점의 이력이며 이 기록의 완료 범위에 한해 갱신된다.

**다음 세션의 첫 행동**: 동일 PHYSICAL을 새 event ID로 middle과 tail에 재전달하는 실패 반례부터
실행한다. 현재 제어 receipt와 head terminal receipt는 physical KV/sampler 재계산을 막는 원장이 아니다.
그 뒤의 순서는 다음과 같다.

1. PHYSICAL 수신/실행의 accepted/running/completed/uncertain 및 stage별 prefix 순서·이전 load/run 차단을
   구현한다. 중복 반환 no-op와 중복 native 실행 no-op를 구분하며 T18/T24를 완결한다.
2. actual `Worker::run` + N-stage 가짜 transport의 지속 입력·출력 포화·취소·재접속·종료를 검사한다.
   메서드 직접 호출 시험을 이 단계의 완료로 치환하지 않는다. 무제한 input drain의 starvation도 여기서 닫는다.
3. reference와 운영의 outcome/stop/position까지 동일 전이로 결속하고, 큰 불변 입력/작은 후보 delta를
   분리한다. full history 스캔/복사 비용을 줄이되 독립 전체 대조·거부 반례·변이를 유지한다.
4. 이후 B3/B4의 실제 KV·row/byte credit와 continuous batching 정책, B5의 나머지 격리·실행 신원·하네스를
   완결하고 B6~B8의 최종 다중 컴퓨터 실기로 간다. 영속 기능을 켤 때만 해당 K 분기 게이트도 선행한다.

실행 소유권 wire가 바뀌었으므로 이전 바이너리/배포를 그대로 사용할 수 없다. 모든 stage가 새 계약을
협상해야 하며 legacy mutation 우회나 mixed-version 자동 허용은 금지다. 실제 모델 선택·다중 물리 호스트·
접근 권한은 여전히 H0 미확정이고, 임의의 작은 모델/한 호스트를 최종 목표로 대체하지 않는다.
native CTest의 exit 0 안에 모델 부재 SKIP 분기가 있으므로 “13개 종료 성공”을 모델 conformance PASS로
읽지 않는다. GPU/실기 웨이브/성능 비회귀는 이번 slice의 완료 주장에 포함하지 않는다.

### 2026-09-07 문서 보강 — 계층 격리는 전 단계의 제약

- 사용자 요구에 따라 P4 공통, adapter L0~L5, native shell/engine bridge/common compat,
  llama.cpp 모델 실행 추상층과 ggml/backend를 구분하는 [격리 계약](layer-isolation-contract.md)을 보강했다.
  새 layer를 추가한 것이 아니라 의존·권한·의미의 세 검사를 명시하고 구현 manifest의 필수 필드를 정했다.
- 기존 코드/CMake의 common signature·Impl 우회·간접 링크·imported 바이너리 신원·handle 수명 경로를
  다시 읽었다. 발견/재확인한 잔여는 격리 계약과 I/T의 하위 시험 요구로 연결했다. 코드 수리나 gate PASS가 아니다.
- 동일 의미의 pin 변경은 허용 compat 모듈 안에서 흡수하고, 정규화 입력/trace와 위층 소스 변경 범위를
  독립 대조한다. 실제 의미·ABI·backend 제약 변화는 거부 또는 명시 계약 변경으로 처리한다.
- **구현 상태는 바로 위 후속 기록 2와 같다.** 이번 보강으로 B1/B2/B5를 완료로 바꾸지 않는다.
  다음 첫 행동은 동일 PHYSICAL의 새 event ID 재전달 반례이며, 이후 순서는 위 기록과 §6을 따른다.
  선언한 최종 모델·호스트·권한이 없는 상태에서 실기나 초대형 모델 성과를 주장하지 않는다.
  현재 load highwater는 같은 Worker 수명 안의 보호다. 같은 agent 내 Worker 재생성도 포함한
  새 load/run 신선성·재연결은 T18/T24 잔여이며, 프로세스만 살아 있으면 보존된다고 해석하지 않는다.
- 문서 검증: `npm run test:docs-lint` 12 passed / 0 failed, `npm run docs-lint` 추적 73파일,
  `node tools/scripts/docs-lint.mjs --all` 전체 79파일 clean, `git diff --check` 오류 없음.
  이번 문서 보강에서는 Rust 전체/C++/GPU 시험을 새로 실행하지 않았다. 이 수치는 I/T/H 구현 통과가 아니다.

### 2026-09-07 후속 구현 — PHYSICAL 재전달과 실기 자원 확정

상태는 **B1/B2/B5 IN_PROGRESS**다. HEAD는 `a9e1967fc`이고 미커밋 변경을 검증했다.
중간/꼬리의 실제 `Worker::handle`→PHYSICAL→native Frame 경로에 수신 원장을 연결했다.
정확한 입력 재전달은 보존 결과를 재생하고, cached+Fresh 혼합은 Fresh만 계산한다. 전체 사전 거부,
native 결과 불명의 fence, 다른 head의 번호 충돌 방지, 해제/슬롯 재사용 후 옛 결과의 비재계산을 검사했다.
정체성/보존 상한/만료의 단독 정의는 [배치 계약](adapter-batching-layers.md)의 PHYSICAL 수신 절을 따른다.
이 구현은 멱등 원장 기반이며 배치 정책의 TPS 개선이나 T24 전체 완료가 아니다.

opaque plan의 move 이후 참조 결함도 시험용 shared preparation에서 고쳤다. 실제 options E2E의
소유권 이전 순서와 모델 없는 lifetime 시험이 같은 helper를 통과하지만, **모델을 적재한 options E2E는
아직 실행하지 않았다**. native 빌드의 종료 성공과 생략된 모델 경로를 별도 집계한다.
최신 시험/RED/변이/소스 식별은 [증거 기록](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)의 마지막 절에 있다.

사용자가 모델 경로·GPU 범위를 지정하고 RAM 오프로딩을 허용했다. 현재 자원과 확장 순서는 §1,
판정 조건은 검증 규약 H0가 소유한다. 목록/SSH 읽기만 수행했으며 새 binary 배포·GPU 모델 load·
실기 웨이브는 실행하지 않았다. 기존 하네스는 GPU 적재·한 ingress를 고정하므로 RAM/다중 호스트
manifest를 이미 실행하는 runner로 취급하지 않는다. B5에서 해당 경로와 부정 시험을 구현해야 한다.

**다음 첫 행동**은 새 execution ID를 붙인 동일 incarnation의 지난 위치·미래 gap·잘못된 phase를
actual middle/tail에 보내는 반례다. 정확한 old ID 재전달과 정상 연속 prefill/decode를 함께 검사한다.
이후 진행은 다음과 같다.

1. ordinary/equal/Verify/Replay와 SETTLE·RELEASE의 stage별 KV frontier를 명시적으로 결속한다.
   지금 소유권 검사는 active owner만 확인하며 이 순서를 증명하지 않는다. 새 Worker/native Session
   생성의 freshness와 credit/retry 수명도 별도로 보강한다. 기존 결과 cache만으로 완료하지 않는다.
2. actual `Worker::run`+N-stage transport에서 발행·부분 반환·지속 입력/출력 포화·취소·drain을 검증한다.
   메서드 시험 14개와 pure 원장 시험을 이 full-loop gate의 대체로 쓰지 않는다.
3. B1 후보의 큰 불변 입력/응답 복사와 `O(seen)` 전체 index 복제를 touched delta로 줄인다.
   독립 전체 원장 대조·원자 거부·변이를 유지한다. cache byte 상한을 active 메모리/RSS 상한으로 확대하지 않는다.
4. B3/B4의 예약·credit·배치 정책과 B5의 native/placement/runner를 완결한 뒤 §1의 자원 단계로 실기에 들어간다.
   부재 자원 때문에 실기 범위가 제한돼도 로컬 정합성 구현을 건너뛰거나 작은 모델로 최종 목표를 닫지 않는다.

### 2026-09-07 후속 구현 — 새 ID의 KV 위치 우회 방어

직전 기록의 첫 행동을 실제 middle/tail에서 실행했다. 정상 경로 1건은 통과했지만 새 ID의
지난 위치·gap·phase 회귀와 혼합 이벤트 사전 거부 8건은 native KV/sampler를 변경하며 실패했다.
이후 순수 stage frontier를 head 발행·중간/꼬리 PHYSICAL·SETTLE/RELEASE에 연결했다.
위치와 phase의 단독 계약은 [배치 계약](adapter-batching-layers.md)의 stage KV frontier 절을 따른다.
정상 Verify 전량 수용·부분 수용·checkpoint Replay도 실제 worker 메서드에서 검사했다.
순수 시험 10개, 실제 PHYSICAL 워커 시험 28개가 통과했고 독립 복사본 변이 6종을 검출했다.
전체 1012 passed / 0 failed / 7 ignored이며 자세한 소스 봉인·원문·변이 범위는 증거 기록의 마지막 절을 따른다.

**B1/B2/B5는 여전히 IN_PROGRESS**다. full Worker::run·native 직접 호출의 위치 자기검증·restart
신선성·credit·VRAM-only/RAM 오프로딩 실기는 완료하지 않았다. 현재 조회한 하드웨어 범위와
VRAM-only 이후 RAM 확장 순서는 §1을 유지하며, 이번 Rust 시험은 실기 성과의 대체가 아니다.

**다음 첫 행동은 새로 재현된 P1 proposal 폭 누락 두 경로를 닫는 것**이다.
`target/proposal-cap-red-20260907-01`의 독립 복사본은 `physical_capacity=2`에서 native의 정상 형식
proposal 3개를 PHYSICAL과 SETTLE 양쪽이 승인하는 반례를 보존한다. 토큰 예산 안이라는 것과
물리 atomic 폭 안이라는 것은 다르다. 이 반례를 원본의 실제 소비 시험에 이관하고, 응답 승인 전에
cap을 대조해 결과 불명 fence·후속 native 0을 확인한다. head의 `SETTLED`가 나중에 거부하는 것으로 닫지 않는다.

그 뒤에는 앞 기록의 full-loop·touched-cost·B3/B4/B5 항목으로 계속한다. 이미 구현된 receipt와
frontier를 다시 처음부터 만들지 않는다. 동결 소스 밖의 새 반례는 whole-suite GREEN에 포함됐다고
보고하지 말고, 해당 원문과 실패 수를 별도로 유지한다. 이번에는 원본 suite가 GREEN이어도 독립 P1 두 건은 RED다.

### 2026-09-07 후속 구현 — continuation 폭과 실제 루프의 첫 검증

직전 P1 두 경로와 정상/잘못된 Fresh 혼합을 원본 시험으로 이관했다. native continuation 폭을
PHYSICAL·SETTLE·head 반환 승인 전에 확인한다. 정상 폭 1/2의 후속 진행과 사후 실패 fence를 함께
검사했으며, 계약의 단독 정의는 배치 계약의 stage KV frontier 절이다.

실제 `Worker::run` 스레드 2/4/8개와 독립 native fake를 연결했다. ordinary 웨이브 합류·개별
token/position·전 stage release·완료 큐 Full 복구·max-open 부분 반환 장벽을 검사한다.
별도 actual EventNode/broker 시험은 outbound Full을 기다리느라 inbound를 못 읽는 두 노드
정지를 재현했다. 중립 코어의 pump는 방향별 이벤트 한 개를 보존하며 반대 방향을 계속 처리한다.
토큰/배치/모델 지식은 코어에 넣지 않았다. 시험 범위와 변이/소스/집계는 최신 증거 기록을 따른다.

**B1/B2/B5는 IN_PROGRESS**다. 위 run-loop fixture는 LOAD/subprocess·실제 모델과 network를
지나지 않으며 ordinary만 다룬다. 별도 broker 시험과 함께 통과해도 통합된 전체 순환망의 credit,
지속 입력 기아, speculative SETTLE/Replay, 취소·graceful drain을 완료한 것이 아니다.
원래 Worker::run의 무상한 입력 drain과 drive 루프는 아직 바꾸지 않았다.

동결 뒤 독립 copy에서 지속 유입 반례를 재현했다. runnable 요청 앞의 Tokenize 연쇄가 0/16/256이면
첫 Logical 이전의 Tokenize 수가 그대로 0/16/256이다. 연쇄 종료 뒤 정상 완주하는 것과 유한 처리
기회를 보장하는 것은 다르다. 이 관측 probe는 원본 whole-suite GREEN에 포함하지 않았다.

**다음 첫 행동:** 위 반례를 원본 실제 run-loop 회귀로 이관하고, 입력 처리와 발행을 유한 기회씩
교대하는 명시 계약을 구현한다. 입력과 발행 양쪽 방향의 기아 및 tail/control의 기회도 검사한다.
새 호출 예산을 제거하면 해당 회귀가 실패해야 하며, 조절 상수를 TPS 최적값으로 발표하지 않는다.
그 뒤 실제 run-loop의 speculative 제어·취소·종료, B1 touched-cost, B3/B4 credit/수용/정책,
B5 native/placement/runner를 진행한다. 하드웨어 자원과 VRAM-only → RAM 오프로딩 순서는 §1을 유지한다.

### 2026-09-07 후속 구현 — 유한 입력/발행 기회와 종료 분류

이 절은 앞의 “무상한 루프 미변경” 기록 이후의 구현 상태다. 실제 `Worker::run`은 한 turn에
입력을 최대 32개 처리한 뒤 head의 자발적 논리 배치를 최대 한 개 발행한다. 발행 성공이면 입력이
없어도 다음 turn에서 재선택하고, gate/무수요이면 입력을 기다린다. 숫자 32는 조절할 TPS 손잡이가
아니라 actor 처리 기회 상한이다. 하나의 PHYSICAL/control 이벤트 안의 작업량·동기 native 호출 시간은 별도다.

관측한 중지/입력 EOF 뒤 새 native 발행을 막는다. 종료 전 로컬 요청·정산·비행·KV/효과 잔량을
남기고, 정상 local empty와 abandoned/stopped/failure를 구분한다. 완료 history는 미완 작업으로
세지 않되 Stopped KV와 Uncertain은 남긴다. cleanup 전 성공 종료를 발표하지 않으며 cleanup 실패는
최상위 실패로 기록한다. 이는 **종료 증거의 보존이지 요청 취소 통보·네트워크 drain의 구현이 아니다.**

지속 Tokenize 연쇄, 입력 없는 추가 발행, 발행 중 정상 SESSION, 중지/EOF/cleanup 실패를 actual
run 회귀로 고정한다. 조회 함수와 실제 종료 소비 시험은 별도로 두고, 구현 제거의 독립 copy 변이·
최종 소스/실행 집계는 날짜별 증거에 남긴다. 기존 GPU 실측을 이 변경의 성능 증거로 재사용하지 않는다.

**B1/B2/B5는 계속 IN_PROGRESS**다. **다음 첫 행동**은 actual run fixture를 speculative
SETTLE/Replay의 정상 전량/부분 수용과 정확한 후속 위치·출력·전 stage release까지 확장하는 것이다.
기존 메서드 직접 호출 시험과 독립 token/KV 모델을 재사용하되 production 전이를 fake 안에 복제하지 않는다.
그 뒤 취소·정상 drain·재시작 없는 반복 실행과 capacity 통지를 검증한다. B1 touched-cost,
B3의 bounded admission/edge row·byte credit, B4의 여러 session을 포함한 공정성/정책,
B5의 native 직접 호출·실제 placement/runner도 남았다. 각 단계의 전체 종료 조건은 §6을 유지하며
이 slice의 성공을 전체 완료로 바꾸지 않는다. 현재 자원에서 VRAM-only의 정상 강한 웨이브를 먼저
증명한 뒤 RAM 오프로딩 모델로 확대한다. 다중 물리 컴퓨터 최종 증명은 별도다.

다음 slice의 시작 파일은 `worker/loop_tests.rs`다. node-target 라우팅 pump는 그대로 사용하고,
ordinary oracle를 약화시키지 않은 별도 speculative oracle를 추가한다. 현재 fake의 `compute`는
Prefill/Decode만 허용하고 `PhysicalSettle`을 구현하지 않았으며, 단순 chunk 분할과 위치당 1회
append 검사는 speculative에 그대로 쓸 수 없다. **전량 수용(SETTLE 없음) / 직접 부분 수용 /
checkpoint 복구 후 Replay** 세 literal 응답 스크립트를 먼저 고정한다. atomic 그룹 보존,
SETTLED를 보류한 동안 새 native 0, append→trim/restore→재append 기록, 정확한 출력 토큰·위치·
상한·종료, 전 stage RELEASE를 독립 대조한다. 특히 Replay의 output flag와 실제 생성 결과를
혼동하지 않으며 rollback 위치는 native 경로와 대조한다. 이것은 다음 시험의 설계 지침이고
현재 1039개에 그 speculative full-loop 시험이 포함됐다는 뜻이 아니다.

### 2026-09-07 후속 구현 — speculative full-loop와 native Replay 경계

앞 절의 첫 행동을 actual Worker::run에서 수행했다. 전량 수용·직접 부분 수용·checkpoint Replay
각각 2/4스테이지, 정산 체인 보류·별도 runnable 요청·정확한 출력·KV 변경 이력·전 stage release를
검사한다. 기존 ordinary 5개를 유지했고 3개를 더했으며 생산 소비 변이 3종이 실패한다.
시험의 단독 판정 조건은 검증 규약 T20/T24, 원문/소스/집계는 최신 정산 증거를 따른다.

fake 통과와 별개로 native 배치가 Replay의 logical output=false를 logits 요청에도 사용하면서
그 logits를 곧바로 읽는 오류를 찾았다. 요청 mask의 native 번역을 logical wire 의미와 분리하고,
실제 배치 생성 본문을 통과하는 모델 없는 소비 시험을 둔다. 이는 실제 llama 샘플링·checkpoint
복원·모델 수치/성능을 아직 증명하지 않는다. FIRST의 서버 capsule mask 복원도 별도 소비 범위다.
또한 no-llama 빌드의 무조건 compat include를 조건부로 고쳤으며 의존 include/link 권한을 넓히지 않았다.

**B1/B2/B5는 IN_PROGRESS**다. **다음 첫 행동은 실행 중 UNLOAD 반례를 닫는 것**이다.
독립 copy actual run은 tail 반환을 보류한 채 기존 UNLOAD를 보내면 native shutdown 1,
거부 0, UNLOADED 1, held tail 1, 출력 0, snapshot unloaded를 기록했다. 이 RED는 원본 전체
1042 GREEN과 별개다. 정지점 밖의 UNLOAD가 미완 요청·KV를 성공으로 지우지 않도록 아래
목표 계약을 실제 경로에 고정한다. 구체적 반례/미실행 양성 대조는 증거 기록에 남긴다.

1. UNLOAD를 정지한 load의 해제로 한정하는 안전 계약부터 구현한다. pending/요청/flight뿐 아니라
   pending SETTLE·RELEASE, Verify fence, middle의 active owner/frontier, Uncertain/효과 잔량도 본다.
   busy 거부는 native 호출·원장 삭제·UNLOADED 성공 효과가 없어야 한다. 이것을 즉시 강제 취소로 부르지 않는다.
2. 원본 ordinary run에 tail 보류 반례를 이관하고, 원래 요청 정상 완주 뒤 idle UNLOAD 성공을
   양성 대조로 실행한다. speculative SETTLED 보류/중간 stage active KV도 추가한다. requests/flight만
   검사하는 잘못된 guard와 무조건 UNLOAD 거부가 둘 다 실패해야 한다.
3. 그 뒤 명시적 Cancel/Drain 상태·권위를 설계한다. 현재 event 어휘에는 둘 다 없고 과거 Agent/deployment의
   Cancel은 다른 경로다. 신규 수용 중단과 기존 반환·SETTLE·RELEASE·출력 전달을 구분하고, 보낸 token을
   되돌리지 않는다. input EOF/Drop·로컬 잔량 0·native unload 성공을 global drain으로 승격하지 않는다.
4. actual EventNode/adapter/transport의 완료 ACK·capacity 통지·종료/join·반복 실행을 결속한다.
   같은 loaded Worker의 slot 재사용, 같은 Worker unload/reload, 새 Worker/agent 재시작의 freshness를
   별도로 검증한다. 현재 load highwater가 새 Worker에도 보존된다고 가정하지 않는다.

이후 B1 touched-cost, B3 bounded admission/edge row·byte credit, B4 다중 session 정책,
B5 native 직접 호출 권위·실제 placement·실기 runner를 진행한다. 미검증 MTP/native model 경로를
ordinary GREEN으로 열지 않는다. 현재 승인된 자원과 **VRAM-only 충분성 검증 → 더 큰 RAM 오프로딩
모델 검증** 순서는 §1과 H0를 유지한다. 단일 호스트 3090×2 성과와 다중 물리 컴퓨터 최종 목표를 구분한다.

### 2026-09-07 후속 구현 — 안전한 UNLOAD와 native 종료 실패 경계

앞 기록의 UNLOAD RED를 원본 actual Worker::run에 이관했다. ordinary tail 보류와 중간 KV,
speculative SETTLED 보류와 중간 Verify KV의 네 회귀가 기존 요청 완주·idle UNLOAD 성공까지
통과한다. 원래 ordinary/speculative oracle는 유지했다. 요청 수만 보는 guard와 무조건 거부도
실패하는 독립 변이를 남긴다. 단독 의미 계약은 배치 계약의 명시적 UNLOAD 절, 판정은 T25가 소유한다.

추가로 idle UNLOAD의 native cleanup 실패 뒤 새 SESSION이 ACK되는 반례를 actual run에서 찾았다.
native 실패 뒤에는 worker를 fence하고 원래 오류를 유지해 종료한다. 정상 busy 거부와 치명적 실패
정리는 구분하며 이 변경은 Cancel/Drain, 실제 OS process 정리 또는 출력 전달 ACK 구현이 아니다.
소스·원문·전체 집계와 변이는 최신 정산 증거에 기록한다. HEAD는 여전히 `a9e1967fc` + 미커밋 변경이다.

**B1/B2/B5는 IN_PROGRESS**다. 다음 소비 경계 감사에서 actual head가 승인한 OUTPUT을
`tools/event-drive/src/run/inference_identity.rs::InferenceIdentity::output`이 꼬리 source만 허용해
거부하는 코드 불일치를 확인했다. 워커 fake 통과를 현재 실기 drive 통과라고 하지 않는다.
독립 copy의 실제 producer 출력 15개(ordinary 5, checkpoint Replay 10)를 그대로 actual consumer에
넣어 모두 거부됨을 재현했다. source만 tail로 바꾼 인과 대조군은 전부 통과했다. 이 변경을 수리로
허용한 것은 아니며 원본 1047 GREEN 밖의 소비자 RED 두 건이다. `target/head-output-consumer-red-20260907-01/verification.md`
및 최신 정산 증거에 실제 캡처/명령/소스 봉인을 남겼다.
**다음 첫 행동은 이 교차 경계 반례를 원본에 이관하고 head 승인 출력 계약에 결속하는 것**이다.
tail/head 양쪽 무조건 허용으로 거부 검사를 완화하지 않는다. 실제 생산 출력 fixture와 strict identity의
load/session/request/route/position 부정 시험을 함께 유지한다. 이는 P4 중립 transport를 바꿀 일이 아니다.

그 뒤 앞 절의 Cancel/Drain 권위·출력 전달/ACK·capacity 통지·종료/join·재시작 없는 반복 실행을
이어간다. cancel 수용·발행 금지·기존 native 정산·전 stage release·OUTER terminal 전달은 서로
다른 증거다. 현재 v2 어휘에 없는 명령을 legacy Agent Cancel로 대체하지 않는다. B1 touched-cost,
B3 admission/edge credit, B4 정책, B5 native/placement/runner와 §1의 실기 순서는 그대로 남았다.

Cancel/Drain의 다음 구현에는 아래 코드상 제약부터 실제 소비 시험으로 닫는다. 현재 명령 구현 사실이 아니다.

- `worker/emit.rs::publish_or_wait`가 완료 Full에서 worker thread 자체를 기다리게 한다. Control class나
  수신 waker만 추가해서 뒤의 취소를 처리할 수 있다고 하지 않는다. bounded effect pump/공간 통지와
  control 처리 기회·용량을 함께 설계하고 T22/T23/T26에서 포화 중 도달을 증명한다.
- admission 전 request-attempt 권위와 admission 후 slot/incarnation·issued membership을 구분한다.
  durable session_key를 취소 ID로 쓰지 않으며 같은 request 이름 재사용 후 늦은 취소가 새 작업을
  건드리지 않아야 한다. 아직 비행 중인 요청을 원장에서 먼저 삭제하지 않는다.
- `event_runtime/transport.rs::deliver_outer`의 enqueue 또는 `write_loop`의 쓰기 성공을 OUTER 소비 ACK로
  쓰지 않는다. 현재 OUTPUT은 incarnation을, RELEASED는 개별 request 완료 watermark를 운반하지 않는다.
  취소 완료/Drain 영수증을 정할 때 이를 명시하며 P4 코어는 모델 의미 없는 전달 계약만 소유한다.

### 2026-09-07 후속 구현 — head 승인 OUTPUT과 실제 OUTER 소비 경계

앞 절의 출력 거부 반례를 원본 기본 시험으로 이관했다. actual Worker::run의 ordinary 및 checkpoint
Replay 출력 15개를 공용 wire fixture로 보존하고, 현재 producer의 의미 대조와 실제 InferenceIdentity/
inference::drive 소비를 각각 연결했다. consumer는 configured head 전체 endpoint만 허용하며 tail이나
모든 node를 같이 허용하지 않는다. 생산자·소비자 변이와 정확한 실행/제외 범위는 최신 정산 증거를 따른다.
승인된 출력 계약은 배치 계약, 판정은 검증 규약 T20 단독 소유다. P4 transport와 native ABI는 이 slice에서
바뀌지 않았다. HEAD는 여전히 `a9e1967fc` + 미커밋 변경이다.

**B1/B2/B5는 IN_PROGRESS**다. source를 고친 actual drive를 독립 copy에서 추가 감사하자 아래
세 거부 반례를 여전히 승인했다. 정상 대조 1 PASS / 거부 규약 3 RED이며, 원본 전체 GREEN과 별개다.

- 같은 요청의 fresh-ID RELEASED 두 개로 전체 해제 수를 채움. 실제 peer의 해제 집합에는 다른 요청이 없음.
- 제출 max_tokens=1인데 연속 위치 OUTPUT 두 개와 length 종료를 승인함.
- 서로 다른 프롬프트의 경계 [4,7]에서 두 번째 요청의 전체 출력 위치를 +1 이동해도 승인함.

**다음 첫 행동은 이 세 반례를 기본 실제 소비 경로 회귀로 이관하고 완료 판정의 증거를 닫는 것**이다.
시작 파일은 `tools/event-drive/src/run/inference.rs`, `inference_identity.rs`, `acceptance.rs`와
`worker/release.rs`다. 재사용할 독립 실행/원본 probe는 정산 증거에서 찾는다. T20의 완료 조건대로:

1. 상한·terminal과 요청별 프리필 경계의 양성/음성 소비 시험을 먼저 고정한다. 기존 response/judge를
   약화하지 않는다. BatchObservation의 도착 순서·유실과 실제 승인된 프리필 끝 위치의 관계를 확인하고,
   선택적 공통 `expected_prefill_rows`를 필수 per-request 증거로 오인하지 않는다.
2. 기존 scalar RELEASED는 요청 집합을 표현하지 못한다. 어댑터 소유 versioned 완료 영수증에 필요한
   membership/operation·incarnation 증거와 실제 producer/consumer 변경을 함께 설계한다. 현재 run 안의
   중복 방지와 재시작 후 freshness를 구분한다. 여러 요청을 한 통지로 해제하는 정상 경로를 유지하며,
   상위의 성공 판정을 단순 count 또는 correlation 중복 제거로 대체하지 않는다.
3. 각 반례가 원본에서 실패→수정 후 통과하고 독립 변이에서 다시 실패해야 한다. producer가 실제로 이
   손상을 만들었다는 주장과, 손상된 peer 응답을 consumer가 막는다는 증명을 구분한다.

그 다음 앞 절의 Cancel/Drain·bounded effect pump·capacity notification·actual EventNode/broker
포화·종료/join·반복 실행으로 이어간다. B1 touched-cost, B3 admission/edge credit, B4 정책,
B5 native/placement/runner는 그대로 남는다. source 승인만으로 GPU 실기나 정상 응답 전체를 승인하지
않으며, 현재 자원의 **VRAM-only 충분성 → RAM 오프로딩 확장**은 §1/H0를 그대로 적용한다.

### 2026-09-07 후속 구현 — OUTPUT 예산과 요청별 fresh-prefill 경계

앞 절의 세 소비자 RED 중 sampled 출력 상한과 요청별 첫 위치를 기본 실제 drive 시험으로 닫았다.
예산 검사는 OUTPUT을 적용하기 전에 수행하고, 요청별 관측 대조는 전체 terminal/해제 경계에서 수행한다.
유효한 OUTPUT 뒤에 관측이 도착하는 순서는 허용하되 그 최종 경계까지 관측이 없거나 어긋나면 거부한다.
빈 EOS도 sampled 예산에는 포함하며, 비어 있는 응답을 정상 품질로 승인하지 않는다. 자세한 의미는
배치 계약의 OUTER 예산 절, 시험은 T20, 실행 원문·독립 변이·봉인·집계는 최신 정산 증거가 소유한다.

이는 fresh position 0 제출의 head 관측과 OUTPUT 대조다. 실제 tokenizer/KV를 독립 관측한 증명이나
Restore/LCP의 위치 계약이 아니다. 공용 actual producer OUTPUT 15개는 그대로 유지했고, producer가
발행한 실제 관측의 prefill 합계를 독립 workload 상수와 비교했다. consumer fixture에 추가한 관측과
해제 통지는 synthetic이며 actual producer 캡처로 부르지 않는다. 전체 Rust **1076/0/7 ignored**,
하네스/build wiring **63/0**는 해당 소스 봉인에만 귀속된다. C++/GPU 실기는 이번 slice에서 실행하지 않았다.

**B1/B2/B5는 IN_PROGRESS**다. **다음 첫 행동은 해제 집합의 실제 생산·소비 계약을 닫는 것**이다.
scalar RELEASED로 A를 두 번 세어 A+B 해제로 승인하는 반례는 아직 미해결이다. 아래 경계들을 같이
다루며, 수신 영수증이 자기 기대 신원을 정하게 하는 순환 검증을 만들지 않는다.
후속 독립 actual run은 서로 다른 OUTER의 A/B가 한 physical capsule에서 정상 종료해도 해제 통지가
A에 2/B에 0으로 가는 RED를 확인했다. 별도 broker→실제 handle 시험에서는 tail이 아닌 middle/외부 Node의
ACK도 슬롯 반환과 대기 요청 재수용을 일으킨다. 두 결과는 원본 1076 GREEN에 포함되지 않은 copy 전용
반례이며 아직 수리하지 않았다. **해제 ACK의 독립 SESSION 권위를 먼저 고정하고**, 아래 외부 완료 계약을
함께 이관한다. 3-stage의 next는 middle이므로 next만 source로 허용하는 임시 수리도 금지한다.

1. 요청 시도·slot/incarnation·release operation의 기대 권위를 해제 통지보다 먼저 확정한다. 기존
   OUTPUT/RELEASED content-type은 이 증거를 충분히 운반하지 않으므로 versioned 어댑터 계약과
   producer/consumer를 함께 바꾼다. 현재 run 중 중복 방지와 새 OUTER/Worker 재시작 freshness는
   별도다. Sender의 sequence는 새 인스턴스에서 1부터 시작하므로 event_id만으로 재시작 신원을 증명하지 않는다.
2. 한 physical batch가 여러 OUTER 소유자를 담는 actual run 반례부터 고정한다. OUTPUT의 소유자별
   ReplySpec처럼 pending release에도 요청 소유 경로를 보존해야 한다. base batch 또는 돌아온 ACK의
   단일 return_route로 여러 소유자의 통지를 보내지 않는다. 내부 multi-request RELEASE batching은 유지한다.
3. 전 stage ACK 검증 뒤 slot 반환·pending admission과 외부 통지의 관계를 명확히 한다. 통지 실패로
   이미 정산된 KV를 다시 해제하지 않으며, 남은 notification intent가 effects에 보존돼야 한다.
   A 유효/B 무효의 원자 거부, 중복/누락/오래된 해제, 다중 OUTER 경로, 첫/후속 emit 실패를 실제 경로로 검사한다.
4. 같은 다중 OUTER actual run의 관측도 소비자까지 연결한다. 현재 전체 batch requests를 모든 ReplySpec에
   보내는 생산과 자기 제출만 허용하는 소비가 불일치한다. 해제 라우팅만 수리한 뒤 다중 OUTER 전체를
   완료로 부르지 않는다. 배치 계약의 소유자별 관측/전체 통계 구분과 T20의 양·음성 시험을 함께 적용한다.
   이미 실제 producer 관측을 사후 수정 없이 실제 InferenceIdentity에 재생하여 A/B 양쪽의 unknown-request
   거부를 확인했다. copy 전용 **1 PASS/1 RED**이며 전체 drive나 원본 기본 시험 완료로 세지 않는다.

이후 Cancel/Drain·bounded effect pump·capacity notification·actual EventNode/broker 포화·종료/join·
재기동 없는 반복 실행을 잇는다. B1 touched-cost, B3 admission/edge credit, B4 정책과 B5 native 권위/
placement/runner도 남아 있다. 모델/GPU 성과는 여전히 §1/H0의 **VRAM-only 충분성 → RAM 오프로딩 확장**
순서에서만 승인한다. 두 GPU의 단일 호스트 검증을 다중 물리 컴퓨터 완료로 바꾸지 않는다.

### 2026-09-07 후속 구현 — SESSION 권위와 해제 ACK 발신자 경계

앞 절의 ACK source 반례를 원본 실제 broker/worker 회귀로 옮기고 SESSION의 독립 토폴로지 선언으로
수리했다. 구 wire를 암묵 변환하지 않으며 OUTER 실제 생산 경로도 함께 이관했다. wire/역할 의미의
단독 정의는 배치 계약의 해제 권위 절, 시험 제약은 T20/T25, 실행·변이·봉인은 정산 증거가 소유한다.
이번 수정은 concrete adapter와 OUTER 안에 있고 P4 중립 broker/native/llama/backend에 지식을 추가하지 않았다.

actual 3-stage 반례는 중간 노드의 ACK로 9번째 대기 요청이 조기 발행되는 것을 구코드에서 확인했다.
수정 후에는 native 발행 상태를 보존하고, 정상 terminal ACK 재개 뒤 기존 출력/위치/KV oracle로
완주한다. 기존 ordinary 2/4/8 및 speculative 2/4 경로를 유지한다. 이는 post-LOAD fake stage의
실제 worker 루프이며 실제 모델/품질/GPU/TPS 증명이 아니다. 코드 기준은 여전히 같은 HEAD의 미커밋 트리다.

code-only 추가 감사에서 transport peer/source 인증·최초 설정자의 control 권한과 SESSION_READY의
전체 topology attest는 별도 미구현임을 확인했다. 계층 귀속은 격리 계약의 신뢰 경계를 따른다.
B5 제품/fleet 신원 게이트에서 신뢰망 제한과 실제 인증/합의 증명 여부를 분리하고, 필드 비교를
발신자 인증 또는 모든 노드의 동일 선언 승인으로 보고하지 않는다. 이 감사는 네트워크 침입 재현이 아니다.

**B1/B2/B5는 IN_PROGRESS**다. ACK 역할 검사는 닫았지만 전체 해제 완료는 닫지 않았다.
**다음 첫 행동은 pending release의 요청 소유 provenance와 versioned OUTER 완료 계약을 연결하는 것**이다.
시작 파일은 `worker/release.rs`, `worker/effects.rs`, `node/state.rs`, `commands.rs`, 그리고
`tools/event-drive/src/run/inference.rs`/`inference_identity.rs`다. 이미 봉인한 다중 OUTER actual run과
scalar 중복 승인 RED를 재사용한다. 소유 route 수정만으로 멤버십 증거·재시작 freshness를 완료로 부르지 않는다.

1. resident 요청 삭제 전에 작은 요청 시도/ReplySpec/slot·incarnation·operation 증거를 pending에 보존한다.
   제출·terminal 승인·해제 통지의 계약과 생산/소비를 함께 이관하며 수신 receipt가 기대값을 만들게 하지 않는다.
2. 전체 ACK 후보 검증 후 상태와 소유자별 notification intent를 함께 commit한다. 실제 Full/Closed/
   event-ID 고갈 반례에서 남은 통지를 보존하고 native release를 반복하지 않아야 한다.
3. A/B 실제 동시 batch의 소유자별 OUTPUT/해제/관측을 실제 OUTER 소비까지 연결한다. 현재 관측의
   foreign-request 거부와 B span 누락도 미해결이다. 전체 통계를 요청 소유 행으로 바꿔 통과시키지 않는다.

그 뒤 Cancel/Drain·bounded effect pump·capacity notification·actual EventNode/broker 포화·종료/join·
반복 실행, B1 touched-cost/B3 admission·credit/B4 정책/B5 native·placement·runner를 잇는다.
실기 자원 순서는 §1/H0 그대로이며 로컬 정합성 GREEN으로 VRAM-only/RAM 오프로딩 웨이브를 대체하지 않는다.

### 2026-09-07 후속 구현 — 요청별 해제 증명과 소유자 통지

정상 sampled 종료에 대해 이전 절의 1·2번을 구현하고 실제 생산/소비를 함께 이관했다. 소유자는 배치
계약의 해제 절이며, 새 wire의 필드·순서·제외 범위를 다른 문서에서 재정의하지 않는다. P4 중립 envelope/
broker와 native/llama/backend는 이번 변경 대상이 아니다. 여전히 HEAD a9e1967fc의 미커밋 작업 트리다.

실제 요청 제출과 terminal 승인으로 기대값을 먼저 고정하여 A의 중복 영수증이 B의 완료를 대신하지
못하게 했다. 실제 2/4-stage 혼합 terminal에서 각 OUTER의 route/correlation/deadline을 보존하고,
첫/후속 Full·Closed·ID 고갈 시 미발행 알림을 보존한다. 실제 원본 PREFILL·OUTPUT·영수증의 새 캡처는
구버전 토큰/text/position/stop 검사를 그대로 유지한다. 이는 post-LOAD fake native와 in-memory
EventWire 증명이지 모델 의미/토큰화·실제 네트워크·GPU 성능 증명이 아니다.

소스379 봉인과 전체 Rust **1122/0/7 ignored**, 확장 JS **75/0**, 독립 생산/소비 변이 및 실행별
범위·처음 실패한 시험은 [정산 증거의 요청별 해제 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)을 따른다.
기존 JS63은 선택된 하네스57+빌드 wiring6의 결과였다. 범위를 넓히면서 오래된 저장소 경로를 참조하던
설정 시험을 발견해 import만 실제 모듈로 고쳤다. 누락 시험을 runtime skip으로 녹색 처리하지 않았다.

**B1/B2/B5는 IN_PROGRESS**다. 현재 run의 정상 종료가 새 OUTER/Worker 재시작 freshness, 출력 없는
Cancel/실패, durable outbox·재연결 수렴 또는 전체 다중 OUTER 성공을 의미하지 않는다. 같은 request_id의
동시 복수 OUTER도 아직 지원하지 않는다. 실패 뒤 fence된 알림은 자동 재발행/회복했다고 보고하지 않는다.

**다음 첫 행동은 같은 실제 다중 OUTER batch의 관측 생산·소비를 맞추는 것**이다. 시작 지점은
`worker/observe.rs`, `commands.rs::BatchObservation`/`StageSpan`, OUTER의 `inference_identity.rs`,
`inference_evidence.rs`, report 집계다. 앞서 봉인한 unknown-request RED와 현재 소유자별 actual run을 재사용한다.

1. 원래 물리 폭·execution 전체 비용과 각 OUTER의 허가된 요청 행을 분리하는 versioned 계약을 고정한다.
   전체 requests를 route마다 복사하거나 unknown-request 거부를 꺼서 통과시키지 않는다. span 수신 대상을
   명시하고 동일 계산을 요청 수만큼 중복 계수하지 않는다. request_id만 있는 관측의 attempt 범위도 감사한다.
2. 실제로 생산된 OUTPUT/영수증/관측을 각 실제 OUTER 소비자에 연결한다. 타 요청 노출·B 관측 누락·
   모든 관측 삭제·전체 통계와 소유 행 혼동의 부정 및 같은 OUTER 여러 요청 양성을 함께 둔다.
   synthetic 관측으로 actual producer의 잘못된 라우팅을 보정해 통과시킨 실행은 이 단계 증명이 아니다.
3. 이후 Cancel/Drain·bounded effect pump·capacity notification·actual EventNode/broker 포화·종료/join·
   재기동 없는 반복 실행을 잇는다. B1 touched-cost, B3 admission/edge credit, B4 정책, B5 native 권위/
   actual placement/runner가 남는다. 과거 U/P 전체를 다시 직렬 선행 조건으로 만들지 않는다.

§1/H0의 **VRAM-only 충분성 → 더 큰 RAM 오프로딩 모델** 순서는 변하지 않는다. 현재 3090×2 한 물리
호스트의 자원 증명과 최종 다중 물리 컴퓨터 증명을 분리하며, 로컬 녹색 수치를 최종 목표 완료로 올리지 않는다.

### 2026-09-07 후속 구현 — 관측 완결 감사와 보고 지표 분리

앞 절의 관측 경로를 실제 코드에서 추가 감사했다. 현재 생산은 전체 요청 관측을 각 route에 복사하고
span은 첫 owner에만 보내며, 소비는 terminal/receipt 후 즉시 종료한다. 받은 execution에만 coverage를
요구하면 관측과 span을 한 묶음 통째로 잃은 경우까지는 발견하지 못한다. 이를 위한 목표 계약은
[배치 계약의 관측 완결 절](adapter-batching-layers.md), 반례는 검증 규약 T20/T25/T57/T58에 고정했다.
이는 **읽기 전용 코드 감사와 계약 보강**이며 새로운 producer/consumer wire 구현이나 GREEN이 아니다.

병행 가능한 보고서 수식 오류를 수정했다. 실제 `run.mjs::buildReport` 소비 경로에 지표 버전을 넣고
계산 행과 승인 출력 토큰을 분리했다. 상세 필드/이전 수치 이관/분모는
[하네스 README](../test/benchmarks/p4-4node/README.md)의 현 구현을 따른다. 새 report 시험11과 독립
변이를 포함한 실행 원문은 정산 증거에 보존한다. 기존 Rust379 봉인은 그대로이고 이번 Rust 전체 재실행은
**1122/0/7 ignored**, 확장 JS는 **86/0**다. 물리 span 집계·Rust 요청별 행 속도·H4 최종 유효 TPS는
이번 수식 수정으로 완료되지 않았다. 실제 모델/VRAM-only/RAM 오프로딩/다중 컴퓨터 실기는 실행하지 않았다.

사용자의 자원 확장 순서는 §1 그대로다. H0는 의도한 CPU 계산이어도 실제 모델 계산/가중치/KV가 host에
의존하면 VRAM-only로 승인하지 않도록 분류를 명확히 했다. 일반 제어/토크나이즈/CPU sampler/staging과
계산하지 않는 비소유 레이어는 구분한다. 기존 runner의 GPU 고정 plan은 RAM offload 지원 증거가 아니다.

**B1/B2/B5는 IN_PROGRESS, 다음 첫 행동은 여전히 실제 다중 OUTER 관측 생산·소비의 결속**이다.
이 보고서 수정은 그 선행 계약을 건너뛰어 GPU 임계값을 다시 튜닝할 허가가 아니다.

1. 배치 계약의 canonical 발행 증거 입력을 독립 literal vector로 먼저 고정하고 `accept_prepared_issue`
   후보 검증/commit과 정상 terminal의 새 OUTPUT 버전에 연결한다. 원장과 요청의 부분 commit·과거
   이력 clone을 금지하고, raw count만으로 동일량의 다른 발행을 승인하지 않는다.
2. 그 계약으로 소유자별 head 관측·fresh stage span과 effect fan-out을 연결하고 actual drive의
   Missing/Invalid/Complete 판정을 구현한다. 기존 실제 A/B 캡처와 구버전 출력 oracle를 보존한다.
   뒤늦은 관측, 묶음 전체 누락, 다른 attempt, 물리 전체/소유 부분 혼동을 실제 경로에서 검사한다.
3. 다음으로 Cancel/Drain·bounded effect pump·capacity notification·EventNode/broker 통합 포화·반복
   실행을 잇는다. B1 touched-cost/B3 admission·credit/B4 정책/B5 placement·runner도 남는다.

현재 Rust/JS 녹색은 정해진 로컬 코드 범위만 증명한다. 모든 신규 목표 시험이 실행됐다는 주장이나
VRAM-only 충분성, 이후 더 큰 RAM 오프로딩 모델의 성공으로 승격하지 않는다.

### 2026-09-07 후속 구현 — 내부 발행 증거와 제출 입구

이전 첫 행동의 **내부 발행 증거만** 실제 L1 승인 경로에 연결했다. canonical 입력·의존 위치·원자성은
[배치 계약의 내부 issued-work v1 절](adapter-batching-layers.md)을 따른다. primitive와 실제 L1 API,
2/4/8-stage actual Worker::run에서 독립 bytes/digest 및 기존 출력/KV/해제 oracle를 대조한다.
승인 기록 누락·조기 commit·execution 입력 누락을 독립 복사본의 재컴파일 변이로 검출했다.

추가 코드 감사에서 P4 envelope와 하위 승인 신원의 NUL 허용 범위가 다름을 발견했다. 실제 worker에서
6개 입력이 계산/KV 변경 뒤 Uncertain·종료로 이어지는 RED를 남겼다. 이를 PREFILL 입구에서 같은 신원
검사로 거부하도록 고쳤다. 정상 Unicode·별도 correlation과 거부 뒤 같은 worker의 정상 재제출을 유지한다.
이 수리는 backend별 코드나 P4 공통 문자열 규칙을 바꾸지 않는다. 범위와 최종 소스/집계/변이 원문은
[정산 증거의 내부 witness 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)을 따른다.

**B1/B2/B5는 IN_PROGRESS**다. OUTPUT v4/관측 wire/actual OUTER 완료 조건은 아직 이관하지 않았다.
현재 witness는 내부 RequestState에만 있고 정상 terminal에서 제거되므로, 이를 외부에서 확인 가능한
증거 또는 전체 관측 누락 검출로 보고하지 않는다. 전체 행 정보·소유 투영·late observation·effect
fan-out의 기존 공백도 그대로다. 수신 관측이 자기 기대 집합을 만들어 승인하는 우회를 금지한다.

**다음 첫 행동은 제출 입구의 남은 row-string 경계를 실제로 고정하는 것**이다. 코드 감사에서
serialized ReplySpec이 logical/capsule의 4096바이트 한도를 넘는 입력은 아직 토큰화/기록 뒤에야
거부될 수 있음을 확인했다. 이는 code-only 열린 표면이며 이번 NUL RED로 실행 재현했다고 하지 않는다.

1. `worker.rs::prefill`과 logical/capsule의 기존 문자열 상한을 비교한다. 원본 JSON 길이와 escape 후
   ReplySpec 바이트 길이를 구분하여 실제 prompt/tokens 입력으로 반례를 먼저 만든다. 정상 경계값·
   Unicode·독립 correlation 양성을 유지하고 토큰화/요청·세션키·slot·witness/다른 요청 효과 전 거부한다.
   기존 wire 상한을 늘리거나 늦은 worker 종료를 정상 요청 거부로 바꿔 보고하지 않는다.
2. 이어서 이미 고정한 witness를 **정상 terminal의 새 OUTPUT 버전**에 복사하고 producer/consumer를
   함께 이관한다. 기존 v4 캡처/출력 oracle는 유지한다. 발행 primitive/실제 L1/worker 변이와 별도로
   실제 OUTER가 최종 count/digest를 대조하는 반례를 추가한다.
3. 소유자별 head 관측·fresh stage span·effect fan-out 및 actual drive의 Missing/Invalid/Complete를
   연결한다. 마지막 관측 지연·중간 관측 묶음 전체 누락·동일량의 다른 execution/위치·타 소유자 누출·
   span 중복/전체와 부분 통계 혼동을 정상 다중 OUTER batch와 함께 검사한다.
4. 그 뒤 Cancel/Drain·bounded effect pump·capacity notification·EventNode/broker 통합 포화·반복 실행,
   B1 touched-cost/B3 admission·edge credit/B4 정책/B5 native·실제 placement·runner를 잇는다.
   SHA 재계산/현재 행 정렬과 기존 프롬프트 clone 비용이 공짜라고 가정하지 않는다.

정책 손잡이를 늘리거나 GPU 사용률 수치로 위 정확성 경계를 건너뛰지 않는다. 현재 자원과 실기 순서는
§1/H0의 **VRAM-only 충분성 → 더 큰 RAM 오프로딩 모델**을 유지한다. 이번에는 모델 적재·실기 웨이브·
원격 배포를 하지 않았으며, 한 호스트의 두 3090과 최종 다중 컴퓨터 증명을 구분한다.

### 2026-09-07 후속 구현 — 제출 문자열 경계와 관측 이관 준비

이전 첫 행동의 serialized ReplySpec/options 크기 경계를 실제 Worker::run에서 RED로 고정했다.
정상 한도 입력은 유지하고 초과 입력은 세션키 기록·Tokenize·요청 수용 전에 거부하도록 공유 wire 검사를
배치했다. 정확한 한도/소유는 [배치 계약](adapter-batching-layers.md), 반례/변이는 검증 규약을 따른다.
Rust와 모델 없는 C++의 기존 codec에도 독립 경계 회귀를 추가했다. 생산 C++/한도/CMake는 바꾸지 않았다.
실행 소스·RED/GREEN·변이·전체 집계는
[정산 증거의 제출 문자열 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)에 보존한다.
기존 Tokenize 실패·context 초과 등 다른 수용 실패의 원자성을 이번 검사로 완료 처리하지 않는다.

**B1/B2/B5는 IN_PROGRESS**다. 내부 witness는 아직 OUTPUT에 없고 전체 관측 누락도 판정하지 못한다.
**다음 첫 행동은 새 OUTPUT와 소유자별 관측 DTO의 실제 생산·소비 이관**이다. 내부 hash만 공개하고
현재 소비자의 terminal/receipt 즉시 종료를 남기는 것으로 완료하지 않는다.

1. 배치 계약의 canonical witness를 재사용해 OUTPUT v5/BATCH_OBSERVATION v4/STAGE_SPAN v4의
   adapter-owned 타입·엄격 검증을 함께 만든다. terminal만 최종 증거를 보유하며 기존 wire를 조용히
   확장하지 않는다. request_issue_index를 쓰면 수신자가 만든 기대 총량이 아니라 head 승인 당시 count로
   결속한다. request가 없는 logical ordinal 간격·역순·정확 재전달을 증분 처리하는 시험이 먼저다.
2. `release.rs`의 RequestState 제거 전 승인 증거를 보존하고, head 관측은 승인된 split+원제출로 만든다.
   full OuterEndpoint별 소유 행과 물리 전체 통계를 분리하고 fresh span만 보존한다. 모든 recipient를
   사전 검사하며 forward 이후 시각은 한 번만 고정한다. 재시도마다 시각을 바꾸거나 native를 재실행하지 않는다.
3. actual drive는 자기 송신 권위·terminal witness·해제 집합·해당 execution의 전 stage coverage를 대조한다.
   Missing은 기존 overall deadline 안에서 기다리고 Invalid는 실패한다. 조기 완료·전체 관측 삭제·다른
   execution/중간 position 교체·다중 OUTER 누출·후순위 Full/Closed/ID 고갈을 실제 producer/consumer로 검사한다.
   기존 원문 캡처/토큰/KV oracle는 보존하며 새 버전은 실제 run에서 새로 캡처한다.
4. 관측을 기다리는 시간이 늘어도 기존 release-boundary 지표의 분모를 몰래 바꾸지 않는다. release 시각을
   별도 latch하고 관측 완결 시각을 분리하거나 summary 버전을 명시 이관한다. owner-visible 작업을 fleet
   전체 비용으로 합산하지 않는다. 정책·원장에 native/backend 타입을 추가하지 않는다.

이후 Cancel/Drain·bounded effect pump·capacity notification·EventNode/broker 통합, touched-cost·
admission/credit·배치 정책·native placement/runner가 남는다. 이번에는 모델·GPU/원격 웨이브를 실행하지
않았다. 현재 자원의 VRAM-only 충분성 이후 RAM 오프로딩 확장 및 최종 다중 물리 컴퓨터 증명은 §1/H0를 유지한다.

### 2026-09-07 후속 구현 — 발행 증거의 OUTPUT·관측 완결 이관

이전 첫 행동의 내부 발행 증거를 실제 생산·소비 경계로 이관했다. OUTPUT v5는 정상 terminal에서만
실제 승인된 witness를 보존하고, head OBS v4와 모든 stage SPAN v4는 full OUTER별 소유 내역을
물리 전체 통계와 분리한다. 버전·필드·정확한 의미의 단독 소유는 [배치 계약](adapter-batching-layers.md)이다.
중립 P4 envelope/core·native stage wire·llama/backend에는 이번 의미 타입이나 의존성을 추가하지 않았다.

actual drive는 실제 송신 권위와 terminal 증거에서 기대량을 얻고 관측의 issue chain/해제/선언 stage
coverage가 끝나기 전에는 성공하지 않는다. 출력·해제 뒤 늦은 관측은 허용하지만 원래 deadline은 늘리지
않는다. release 완료 시각과 관측 완결 시각을 분리해 기존 처리량 분모를 보존했다. 기존 v3/v4 원문과
token/text/position/stop·KV·해제 검사는 유지하고, 새 wire는 actual worker에서 따로 캡처했다.

검수 중 정상 span에 근거 없는 empty-owner 실행을 더해도 성공하던 반례를 실제 drive에서 재현했다.
이제 **수신된** global 실행은 소유 내역이 비어도 head 물리 크기로 확인될 때까지 Missing이다. A에게
아예 도착하지 않은 B-only stage span을 요구하는 규칙이 아니다. 정상적인 span-before-head도 유지한다.
정확한 실행 소스·수정 전 실패·독립 복사본 변이·전체 집계와 한계는
[정산 증거의 OUTPUT·관측 이관 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)에 보존한다.

**B1/B2/B5는 IN_PROGRESS**다. 이번 검사는 현재 run의 소유·관측 완결이지 실제 llama tokenizer/품질,
GPU wavefront, 교차 호스트 시계, 전체 통계의 독립 장치 계측 또는 완전한 분산 drain의 증명이 아니다.
고정 native 응답을 쓴 actual worker와 캡처를 재생한 actual drive를 구분한다. offline acceptance는
온라인 원장 검사를 독립 재실행하지 않는다. target의 로컬 봉인은 영속 배포 bundle도 아니다.

**다음 첫 행동은 completion Full 중 실제 정상 정산 ACK가 멈추는 반례를 고정하는 것**이다.
`worker/effects.rs::flush_effects` → `worker/emit.rs::publish_or_wait`가 worker 스레드에서 대기하므로
기존 input/issue quantum도 제어 이벤트를 읽을 수 없다. 단순히 sleep을 waker로 교체하는 것만으로는
같은 actor의 제어 소비 기회가 생기지 않는다. 아래 순서로 진행한다.

1. 실제 worker에 A의 RELEASED를 보류하고 B의 완료로 큐를 포화시킨 뒤 A의 정확한 ACK를 보낸다.
   시험용 읽기 관측으로 실제 입력 수용·포화·미처리를 구분한다. 원인 재현 뒤 출력 공간을 다시 열어
   원래 OUTPUT/receipt/관측의 바이트·순서·한 번 전달과 전 stage 정산을 유지하는 양성도 실행한다.
   source 변화 없는 시간 경과만으로 포화/기아를 선언하지 않는다.
2. immutable committed intent를 보존하는 bounded effect pump와 capacity 통지를 설계한다. Full은
   미발행 작업 유지, Closed/ID 고갈은 명시 실패이고 native 재실행이 아니다. 효과 의존 순서와 정산
   authority를 지키면서 제어 소비 기회를 부여한다. 무상한 옆 큐·우회 슬롯 반환·ACK=KV완료 대체는 금지한다.
   공용 mailbox 변경은 mock/다른 adapter의 중립 계약과 lost-wakeup·close·실제 양방향 포화를 함께 검사한다.
3. 실제 EventNode/broker의 입력 종료와 이미 수용한 늦은 완료를 연결한다. 현재 local-close의 Ok를
   graceful drain으로 부르지 않는다. adapter-owned Cancel/Drain의 권위·수용 ACK·발행 금지·기존 native
   정산·전 stage release·OUTER 최종 전달을 나눠 계약/버전화하고, 출력 없는 취소와 전후 중복·재시작도
   검사한다. 코드에 없는 event 명령을 legacy service 명령으로 대신하지 않는다.
4. B1 touched-work/불변 입력 clone 비용, B3 bounded admission/KV 예약/row·byte credit, B4 정책,
   B5 실제 placement/ABI·격리/runner를 각각 해당 gate로 이어간다. 관측을 모두 메모리에 보존하는
   드라이브 원장을 bounded-RSS 증명으로 세지 않는다. 오류 시 partial artifact 저장도 아직 별도 작업이다.

이번 slice의 convenience `SubmissionLedger::approve_output`는 실제 drive가 아닌 시험만 사용하는
미사용 경고가 남는다. 증거를 위해 동결한 소스를 마지막에 몰래 청소하지 않았으며 후속 코드 편집에서
범위를 축소하고 재검증한다. 모델 적재·GPU/원격 배포·VRAM-only/RAM 오프로딩은 이번 slice에서 하지 않았다.
현재 fleet의 **VRAM-only 충분성 → 더 큰 RAM 오프로딩 모델** 순서와 최종 다중 컴퓨터 증명은 §1/H0 그대로다.

### 2026-09-07 후속 구현 — completion Full의 실제 반례와 공간 통지

직전 첫 행동을 actual Worker::run에서 재현했다. A의 native 해제 뒤 정확한 ACK를 보류하고 B의
OUTPUT으로 completion을 채우자 A ACK는 입력에 수용돼도 처리되지 않았다. 공간 복구 뒤 기존
출력·해제·관측은 모두 완결됐다. 새 필수 시험은 숨기거나 기대값을 낮추지 않고 **RED로 남긴다**.
원본 소스·실제 재컴파일·실행파일과 복구 양성은
[정산 증거의 completion Full 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)에 보존한다.

중립 mailbox의 공간 통지와 reader 종료 wake는 이 actor 문제의 선행 작업이다. 그 primitive의
구현·검증 결과는 같은 증거 절을 따른다. 이를 추가해도 현재 `publish_or_wait`의 blocking이나
EventNode의 입력 재시도·graceful drain이 사라지지 않는다. **B1/B2/B5는 IN_PROGRESS**이며 이
RED가 남아 있는 동안 이전 전체 green 집계를 현재 집계로 인용하거나 성능 단계로 승격하지 않는다.

**다음 첫 행동은 bounded effect pump를 실제 worker에 연결하여 이 RED를 통과시키는 것**이다.
정확한 목표 의미는 [배치 계약의 출력 포화 절](adapter-batching-layers.md), 시험은 검증 규약
T22~T26이 소유한다. 이미 재현된 ACK 기아를 새 발견처럼 다시 조사하거나 환경변수로 우회하지 않는다.

1. head 제어 의도의 적용/전송 단계를 먼저 명시화하고 조기 ACK 거부 반례를 추가한다. 기존 normal
   RELEASED·terminal proposal이 붙는 SETTLED·checkpoint replay를 함께 유지한다.
2. 직접 LOAD/SESSION/UNLOAD/오류 응답까지 동일한 고정 송신물·효과 예약 경계로 통합한다. count/byte
   예약은 전체 후보 검증 뒤 commit 전에 확보하고, native 결과를 보존할 공간은 호출 전에 확보한다.
   기존 wire 한도를 임의 축소하거나 다른 무상한 큐로 옮기는 것은 이행이 아니다.
3. 입력·capacity·shutdown을 함께 기다리는 actor loop를 연결한다. 정상 ACK는 진행하되 새 native
   발행으로 backlog를 키우지 않는다. 효과의 일부 native 성공 뒤 Pending/Closed/ID 고갈·종료에도
   원본 Event/순서·한 번 실행·불확실/abandonment 회계를 지킨다. capacity API 존재만으로 완료하지 않는다.
4. actual EventNode/broker 포화와 명시 Cancel/Drain을 이어 검증한 뒤, 남은 B1 touched-cost,
   B3 admission/예약/edge credit, B4 정책, B5 placement/ABI/격리/runner를 진행한다.

이 slice는 모델 적재·GPU/원격 배포·실기 웨이브를 수행하지 않는다. §1/H0의 사용자 지정 자원과
VRAM-only 이후 RAM 오프로딩 확장 순서는 그대로다. 한 물리 호스트의 두 GPU를 다중 컴퓨터로 세지 않는다.

### 2026-09-07 후속 구현 — head 제어의 적용·전송 권위

직전 첫 하위 작업인 head 제어 단계를 구현했다. pending 등록만으로 조기 ACK가 슬롯을 반환하거나
Verify/Replay를 재개하는 반례를 실제 codec→handle에서 수정 전 RED로 보존했다. local native 성공과
다음 stage 송신 수용을 별도 상태로 결속했고, 기존 꼬리 proposal/Replay와 whole-event 거부를 유지했다.
실제 효과 소비 시험은 native Frame 응답·receipt/frontier·송신 수용을 따로 통과한다. 초기 상태를
주입한 소비 시험과 실제 run-loop 진행 증거는 구분한다. 의미·ticket 수명은 [배치 계약](adapter-batching-layers.md),
시험 의무는 검증 규약 T23, 봉인·실행·변이는
[정산 증거의 head 제어 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)이 소유한다.

**B1/B2/B5는 IN_PROGRESS**다. 실제 completion Full의 ACK 기아 시험은 여전히 필수 RED이며,
blocking publish·capacity API의 미연결·전체 outbox 예산·Cancel/Drain은 이번에 고치지 않았다.
phase 필드가 존재한다고 actor가 양보하거나 전송 credit이 KV 정산을 증명하는 것은 아니다.

**다음 첫 행동은 고정 송신물과 전체 효과 예약을 실제 소비 경계에 연결하는 것**이다.

착수 반례는 독립 복사본의 `SESSION` 응답 ID 고갈로 고정했다. `control.rs::Worker::session`이
`emit.rs::Worker::emit_bytes`의 ID 검사보다 먼저 sessions.insert를 수행해, 거부 후에도 route가
설치된다. 실제 session()/handle() 두 경로는 RED, 정상 응답 wire 양성은 통과했다. 원본 전체1225 집계에
이 복사본 시험을 합산하지 않는다. 우선 이 반례를 기본 회귀에 이관해 **SESSION 응답 준비·예약 전
상태 변경 금지**를 닫는다. 이 작은 소비 경계에서 시험한 ID/송신물 준비를 native 결과 예산이나
전체 outbox 예약 완성으로 부르지 않고 아래 전체 경로로 이어간다.

1. `worker/effects.rs`, `emit.rs`, `control.rs`, `drive.rs`의 모든 직접 송신을 포함해 보존 비용과
   예약 수명을 감사한다. 이미 발행한 작업이 나중에 만드는 OUTPUT/ACK receipt의 용량은 발행 전에
   확보해야 한다. ACK 도착 때 처음 예약하면 다른 OUTPUT이 예산을 채워 같은 기아가 재발한다.
2. 전체 후보의 count/retained-byte/ID를 상태 commit 전에 예약하고, 한 번 만든 Event를 그대로
   Full 재제출한다. 현재 native frame 상한과 결과·관측 fan-out·base/payload 복사본을 제외하지 않는다.
   결과를 받은 뒤 임의의 작은 cap으로 버리거나, 고정 크기 큐만으로 RSS 상한을 주장하지 않는다.
3. 그 예약을 소비하는 비동기 effect pump에서 입력/capacity/shutdown을 함께 기다린다. 동기 구간용
   ticket은 양보 뒤 재사용하지 않고 재검증한다. native command 내부 양보에는 별도 그룹 예약이 먼저다.
   실제 Full ACK RED와 정상 전달 양성을 모두 유지해 통과시킨다.
4. 이후 순서는 직전 진행 기록의 EventNode/broker·Cancel/Drain 및 B1/B3/B4/B5 잔여를 따른다.

소스는 기준 HEAD 위 미커밋 변경이다. 모델/GPU·C++/원격 실행·배포·커밋/push는 이 slice의 성과가
아니다. §1/H0의 **VRAM-only 충분성 검증 후 RAM 오프로딩 모델 확장**과 다중 컴퓨터 최종 증명은 유지한다.

### 2026-09-07 후속 구현 — SESSION 응답 준비의 소비 경계

직전 첫 반례를 기본 SESSION 회귀로 이관했다. 응답 직렬화·ID 후보·실제 Event 왕복 가능성을
상태 변경 전에 확인하고, 성공했을 때만 session 설치→ID commit→기존 동기 송신으로 진행한다.
입력은 wire-valid지만 원본 ID가 응답 ID/causation에 중복돼 응답 envelope가 커지는 추가 반례도
독립 복사본에서 재현했다. 단순 Event::validate 또는 encode 성공으로는 decoder 승인까지 보장되지
않는다. 기존 정상 wire와 Unicode metadata/body를 유지했다. 정확한 시험/봉인/집계·변이는
[정산 증거의 SESSION 응답 준비 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)이 소유한다.

**B1/B2/B5는 IN_PROGRESS**다. 이 private 준비물은 같은 worker 동기 구간 전용이며 queue count/
retained-byte/미래 native 결과 예약이 아니다. 전체 wire 한도를 줄이거나 중립 protocol을 고친 것이
아니며, 일반 ERROR/LOAD/UNLOAD 응답에는 아직 이 준비 경계가 없다. 큰 원본 ID의 ERROR fallback도
wire-invalid일 수 있다. 준비 실패 시 SESSION 권한 보존과 정상 오류 응답 전달을 구분한다.
현재 completion Full ACK 기아는 필수 RED 그대로이며 Closed 뒤 session 롤백도 이번 범위가 아니다.

**다음 첫 행동은 효과의 보존 표현과 예산을 실제 발행/정산 의무에 결속하는 것**이다.

1. `effects.rs`에서 OUTPUT마다 전체 TAIL base Event를 복제하고 flush에서도 front를 clone하는
   비용부터 명시·축소한다. envelope만 필요한 곳은 본문 전체를 중복 보관하지 않도록 하되 실제
   OUTPUT/관측/제어 Event의 identity·본문·순서가 동일한 소비 회귀를 먼저 고정한다.
2. `release.rs`의 전체 TAIL 효과 및 **앞으로 올 RELEASED가 생성할 소유자 receipt**를 함께
   예약하고, `drive.rs`의 native 호출 전에 결과/forward/관측 의무 상한을 예약한다. planned 예약과
   실제 발행 witness는 분리한다. ACK 도착 후 일반 예산을 처음 요구하는 구조는 허용하지 않는다.
3. count뿐 아니라 보존 bytes·중첩 telemetry fan-out·파싱/Vec capacity·일시 복제를 계측한다.
   현재 frame의2GiB 한도를 RSS 한도로 사용하지 않는다. `capsule/decode.rs::read_capsule`의
   선언 count에 따른 선할당도 예산/유효 입력 길이와 대조해야 하며, 실제 대용량 할당으로 개발 호스트를
   고갈시키는 반증은 금지한다. 협상된 상한 또는 안전한 격리·계측을 사용한다.
4. 모든 직접 응답과 미래 의무가 같은 보존 경계에 들어간 뒤 capacity/input/shutdown actor pump를
   켜서 기존 Full ACK RED를 통과시킨다. 이후 EventNode/broker·Cancel/Drain과 나머지 B1/B3/B4/B5를
   잇는다. 일반 ERROR의 미이관을 잊고 정상 token 경로만으로 포화 해결을 선언하지 않는다.

실기 확장 순서는 §1/H0 그대로다. 이번 모델 파일 목록은 읽기 전용 경로/stat 조사이며 load·GPU·
VRAM-only/RAM 오프로딩 웨이브 실행은 하지 않았다. 소스는 미커밋 변경이며 자동 push/배포는 하지 않는다.

### 2026-09-07 후속 구현 — 효과 보존 표현과 할당 전 검사

직전 첫 작업인 효과 표현을 실제 생산·소비 경로에서 이관했다. OUTPUT뿐 아니라 Forward/관측/
해제 통지의 provenance도 Envelope만 소유하며, flush는 전체 effect를 clone하지 않고 소유권을
옮긴다. 실패하면 본문과 중첩 관측을 원본 의도로 복구한다. 캡슐의 잘못된 outcome/generated 선언이
본문도 없이 Vec를 선할당하던 경로는 최소 wire 크기 검사로 막았다. 정확한 코드 봉인·시험·변이는
[정산 증거의 효과 보존 표현 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)이 소유한다.

**B1/B2/B5는 IN_PROGRESS**다. 실제 completion Full ACK 기아는 여전히 필수 RED다. 보존 표현의
복사 감소는 전체 RSS 상한·고정 Event 재개·처리량 개선·비동기 pump 완성의 증명이 아니다.
활성 입력/파싱 객체·일시 직렬화·실행 중 effect·미래 결과와 통지 의무는 별도로 회계해야 한다.

**다음 첫 행동은 Worker 수명의 ResourceBudget을 실제 응답/발행/반환 소비에 연결하는 것**이다.
상세 예약·ID·수명 계약은 [배치 계약의 출력 포화 절](adapter-batching-layers.md), 실패 반례는
검증 규약 T22~T26이 소유한다. 이미 완료한 표현/검사 작업을 재감사만 하며 반복하지 않는다.

1. composition root→adapter 생성 설정에 자원 선언을 전달하고, Worker 소유 예약 원장을 연결한다.
   일반 보존물·입력·native 임시 공간·미래 반환·실패 진단을 합산한다. queue 개수나 native frame
   한도로 RSS를 대체하지 않는다. 실제 response/native 후보의 예약 실패가 상태·ID·효과를 보존하는
   소비 시험을 함께 넣는다. 사용하지 않는 순수 budget 클래스만 만든 것으로 이 단계를 닫지 않는다.
2. TAIL 후보의 pending release에 미래 owner receipt와 ID 발급 개수 예약을 귀속한다. 원본 제출/
   operation별 권한을 검사하고, 정상 ACK가 일반 예산이 찼어도 자기 예약으로 정산하도록 한다.
   잘못된 ACK·전체 그룹 초과·ID 여력 경계·중복 전환·native 불확실 상태를 실제 consumer로 검증한다.
3. 직접 LOAD/UNLOAD/SESSION/ERROR와 native 결과/forward/관측까지 같은 보존 경계를 완성한다.
   그 뒤 실제 actor가 입력/capacity/shutdown을 함께 처리하게 하고 기존 Full ACK RED를 통과시킨다.
   단일 FIFO 앞단 포화와 이미 수용된 ACK 진행을 구분하며, native 그룹 내부에는 아직 양보하지 않는다.
4. 실제 EventNode/broker 포화·Cancel/Drain 및 B1/B3/B4/B5 잔여를 잇는다. 예약된 반환 입력 경로/
   edge credit 없이 전체 순환망 진행을 승인하지 않는다. 안전성 승인 후 B6/B7 실기 웨이브로 넘어간다.

§1/H0의 **VRAM-only 충분성 검증 뒤 RAM 오프로딩 모델 확장**은 유지한다. 이번 slice에는
모델 적재·GPU 웨이브·원격 배포·C++ 실행·커밋/push가 없다. 최종 다중 컴퓨터 증명도 아직 아니다.

### 2026-09-07 후속 구현 — ACK 진행 설계 재검수와 전체 체크포인트

사용자가 과도한 시간·토큰과 국소 수리의 반복, 장기간 무커밋을 지적했다. 전체 원인을 다시
대조한 결과, 직전 기록의 **전체 ResourceBudget을 현재 ACK 정체 수정의 직렬 선행으로 둔 결정은
과했다.** 전체 RSS·native 임시 공간·EventNode credit는 필요하지만 이 국소 반례의 선행 조건은 아니다.

실제 반례에서 mailbox를 점유한 것은 이미 전송된 B OUTPUT이며, worker가 대기하는 것은 그 뒤
B의 RELEASE forward다. A는 이미 ForwardAccepted이고 정상 ACK가 입력에 들어와 있다. 해결의
최소 단위는 다음 전이를 함께 보존하는 것이다: 동일 활성 Event 유지 → 접근 가능한 ACK의 순수
prepare/commit → 기존 FIFO 뒤 receipt/진단 보존 → 매 offer 직전 head 권위 재검증 → 성공 callback.
native 그룹 내부 선점·재귀 handle/flush·추가 native issue는 허용하지 않는다.

현재 작업 트리에 이 제한된 서비스와 queued/active-suffix/future-receipt ID 개수 대조를 통합했고,
전체 누적 소스·시험·문서를 **WIP 체크포인트**로 함께 커밋한다. 정확한 소스 봉인·전체 집계와 아직
미통과한 회귀는 [정산 증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)의
마지막 체크포인트 기록이 소유한다. 이전 전체1244/1/7을 새 수정의 결과로 재사용하지 않는다.

**B1/B2/B5는 IN_PROGRESS**다. 정상 ACK는 pending 권위의 수만큼만 새 receipt를 만들 수 있고,
추가 일반 보관은 FIFO 입력1개·진단1개로 제한한다. 이것은 추가 객체 수에 대한 구조적 한계이지
전체 byte/RSS 예약 또는 native 응답 보관 상한이 아니다. native frame 한도를 RAM 예산으로 쓰지 않는다.
non-ACK가 FIFO 앞을 막거나 두 번째 잘못된 ACK를 보관한 이후의 ACK 진행은 이 국소 보장의 범위 밖이다.
1ms 대기·capacity/input 통합 wake·완전한 고정 outbox·전 경로 byte 예산·Cancel/Drain도 완료하지 않았다.

다음 첫 행동은 **아래 하나의 수정 묶음을 증명하는 것**이다. 새 성능 손잡이나 일반 자원 모델을
먼저 추가하지 않는다. 실패에 맞춰 기존 정상/거부 기대값을 약화하지 않는다.

1. 기존 actual Worker Full ACK 반례, 조기 ACK/전체 그룹 거부, 정상 복구를 그대로 통과시킨다.
2. 같은 실제 하네스에서 잘못된 ACK1개 뒤 정상 ACK, non-ACK 보관/FIFO 복구, SETTLED의 직접
   proposal/Replay를 확인한다. 정상 ACK 정산이 새 native 실행을 일으키지 않는지도 동시에 단언한다.
3. 동일 송신물 재시도·매번 fresh head ticket·ID 미래 의무/queued suffix·overflow/고갈·종료 잔존을
   실제 소비 경로와 독립 복사본 변이로 검증한다. 컴파일 오류나 미실행은 변이 검출이 아니다.
4. 봉인된 최종 전체 시험·변이·실행 범위와 남은 한계를 기록하고 전체 변경을 다시 커밋한다.
   이후 capacity wake/반환 수용 경로·byte 예산·B3/B4/B5 및 승인된 VRAM-only→RAM 실기를 진행한다.

어떤 입력·장애에도 예외가 없다는 전역 보장은 하지 않는다. 각 보장은 상태·실패 모델·진입 경로와
경계 밖을 함께 명시한다. 중간 커밋 이후 비무시 변경0을 확인하며, GPU·배포·push는 이번 체크포인트의
검증이나 권한에 포함하지 않는다.

### 2026-09-07 후속 구현 — 제한된 ACK 진행 검증과 두 번째 전체 체크포인트

누적 변경196파일을 `2e9451a5c` WIP로 먼저 커밋하고 비무시 잔여0을 확인했다. 이어 그 체크포인트의
회귀9개를 수정하고 실제 소비 경로 시험8개를 추가했다. commit 전 의무 검사와 commit 후 전달 장애를
분리했으며 기존 후자 시험의 기대값은 유지했다. native 발행의 ID 거부도 prepared issue 설치 **전**이다.
전체399 Rust 입력 봉인에서 **1253 passed/0 failed/7 ignored**, 57 summary·cargo0이다.
수정5종을 독립 복사본에서 제거하면 모두 실행된 시험이 실패하고 복원하면 다시 통과한다.
정확한 입력·시험명·명령·변이·해시는
[정산 증거의 두 번째 체크포인트 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)이 소유한다.

**이번에 닫힌 반례**는 동일 worker의 completion Full 때문에 FIFO에서 접근 가능한 정상 RELEASED/
SETTLED가 정산되지 않던 국소 기아다. 실제 worker에서 오류 ACK1개 뒤 정상 ACK, non-ACK의 원본 보관·
순서 복구, Direct/Checkpoint SETTLED의 공간 복구 전 정산과 native 재진입0을 검사했다. 별도 실제
flush 시험은 Full 도중 ACK로 권위가 은퇴한 제어 재전송을 재검증해 거부하고 원래 의도를 보존한다.
이 마지막 경우는 성공으로 흡수하지 않고 fenced 상태로 남긴다. 완료된 재전송의 투명한 회수는 미구현이다.

**B1/B2/B5는 여전히 IN_PROGRESS**다. 이 변경은 byte/RSS 예약, 모든 입력에서의 진전, 완전한 outbox,
전송/노드 순환망의 credit, graceful Cancel/Drain 또는 성능 개선을 완성하지 않는다. 두 번째 오류나
non-ACK 뒤의 ACK를 추월하지 않으며1ms 대기도 남아 있다. 이 경계를 숨기고 “포화 해결 완료”라고 하지 않는다.

다음 세션의 첫 작업은 **현재 정상인 원장/정산을 다시 만드는 것이 아니라 반환 수용 경로를 검증하는 것**이다.

1. 실제 EventNode→adapter 입력→completion→broker의 소유·용량·wake 흐름을 한 표로 고정한다.
   non-ACK 앞단과 출력 Full이 겹친 최소 순환 대기 반례를 먼저 만든다. 아직 관측하지 않은 전역
   deadlock을 이번 국소 반례에서 추론해 확정하지 않는다. ACK 전용 수용/예약 경로와 FIFO 계약을 함께 결정한다.
2. 입력/capacity/shutdown 통합 wake와 반환 경로를 그 반례에 연결한다. 임의 sleep/threshold 추가,
   ACK의 native 재진입, 일반 요청 무제한 drain으로 해결하지 않는다. 거부 보존·중복·재연결·종료와
   정상 혼합 요청 완주를 같은 실행에서 검사하고 독립 변이로 고정한다.
3. 활성 Event·보류 입력·효과·미래 반환/영수증·native 임시 공간을 포함한 byte 예산을 실제 경로에
   연결한다. 현재 ID 개수 검사를 byte 예약으로 이름만 바꾸지 않는다. B3/B4/B5 잔여를 검증 규약에
   따라 통과한 뒤 B6/B7의 승인된 **VRAM-only→RAM 오프로딩 강한 웨이브**로 넘어간다.
4. 각 응집된 변경과 검증 결과를 전체 중간 커밋으로 남긴다. 비무시 잔여0을 확인하고, 실패한
   체크포인트도 실패 그대로 표시한다. build/모델/원문 실행물은 ignore, 소스·시험·문서는 누락하지 않는다.

이번 두 번째 체크포인트도 C++·실제 모델·GPU·원격 배포·push를 실행한 것은 아니다. 초대형 모델의
정상 프롬프트/응답 전문과 유효 TPS·GPU 활용의 최종 실기 목표는 §1/H0/H6 그대로 남아 있다.

### 2026-09-07 후속 감수 — 외부 감수 대조와 작업 범위 재점검

외부 감수의 11:43~11:45 스냅샷과 **1236/9/7**은 첫 WIP 시점의 결과다. 이후 전체 체크포인트
`96c90f99e`는 **1253/0/7**이다. 9개 회귀를 기대값 완화로 숨기거나 ResourceBudget 완료로
기록하지 않는다. 현재는 접근 가능한 ACK의 국소 진행과 ID 개수 의무 검사이며, B1/B2/B5는
IN_PROGRESS다. 정확한 실행과 이번 작은 API 정리의 결과는
[정산 증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)가 소유한다.

이번 범위 심사에서는 생성 원자료 묶음과 특정 커밋 전용 보관 도구의 Git 포함을 **철회**했다.
이미 커밋된 소스의 반복 해시 목록을 추가해도 다른 머신에서의 실행 재현은 성립하지 않는다.
삭제하지 않고 무시 경로에 보존하며, 장기 증거 보존/재실행 조건은 미충족으로 남긴다.
보관 도구 개발로 배치 작업을 확대하지 않는다. 전체 커밋은 무시 대상 외의 소스·시험·문서를
빠짐없이 포함한다는 뜻이며 생성물까지 무차별로 넣는다는 뜻이 아니다.

**다음 첫 행동은 앞 기록의 반환 수용 경로 검증 그대로**다. 실제 EventNode·broker·Worker를
잇는 최소 반례와 정상 진행 oracle부터 고정한다. 코드상 후보는 정상 추론과 허용된 SESSION
재전달이 각 입력/출력 큐를 함께 채우는 순환 대기다. 아직 실행된 RED가 아니며 순수 PREFILL
웨이브만의 결함이라고 확정하지 않는다. 기존 duplex 시험의 어댑터는 항상 입력을 받거나
시험이 외부에서 여유를 주므로 이 후보를 증명하지 않는다.

시험 전에 각 보류 Event의 소유자·큐 상한·wake·대상과 유한 도달 순서를 기록한다. 재현되기 전
새 ACK lane/예약 정책/임계값을 구현하지 않는다. 재현 후에도 정책/원장과 llama/backend 경계를
유지하며, 정상·포화·종료의 수용 조건을 먼저 정한다. 단계 전환 시
[검증 규약의 시행착오 점검](distributed-batching-verification.md)을 적용한다. 현재의 국소 수정을
반복 재작성하거나 GPU 실험으로 이 정합성 설계를 찾지 않는다.

### 2026-09-07 후속 검증 — 실제 actor 순환 반례와 수정 경계

기준은 `f13e2560b`다. 운영 변경 없이 실제 EventBroker→EventNode→LlamaNodeAdapter→Worker::run의
시험 두 개를 추가했다. native 계산/유한 지연과 post-LOAD 초기 설정만 fake이며 정상 SESSION·
추론·캡슐·해제 상태는 실제 경로에서 만든다. 이전 감수의 실행은 보강 전 소스이므로 현재 소스의
결과로 재사용하지 않는다. 정확한 실행·실패·보류 소유자 표는
[정산 증거의 actor 순환 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)이 소유한다.
봉인400입력의 실행13은1254/1/7 ignored·cargo101이며 cap1 마지막 진행 단언만 실패하고 cap8은
통과했다. 운영 변경 없는 수정 전 RED 체크포인트다. 이전1253/0/7을 현재 상태로 읽지 않는다.

이번 판정은 B2/B3의 **수용 경로와 원인 작업의 후속 공간 보장**이다. 제한된 ack_service를 넓히거나
SESSION을 금지하고, 큐/임계값을 늘리거나, GPU A/B로 정체 원인을 다시 찾는 작업이 아니다.
capacity wake는 비워진 공간을 알릴 뿐 이미 닫힌 대기 고리에 공간을 만들지 못한다. 같은 source/
correlation의 순서와 필수 결과의 공간을 함께 지켜야 하므로 ACK 우선 lane 하나로 완료할 수 없다.
의미 계약은 [배치 계약의 출력 포화 절](adapter-batching-layers.md), 중립 경계는 격리 계약이 소유한다.

**B1/B2/B5는 IN_PROGRESS, B3 end-to-end 예약은 미완**이다. 다음 첫 행동은 봉인 반례를 별도
체크포인트로 남긴 뒤 다음 구현 범위를 닫힌 상태 전이 표로 확정하는 것이다.

1. 원인 작업의 승인 전에 필수 결과·반환·앞선 동일 순서 송신물의 count/retained bytes를 선확보한다.
   adapter는 flight/KV 의미를, 중립 전달층은 공간/소유를 책임진다. 일반 입력이 예약된 반환의
   공간을 소비하지 못하게 하고, 모든 일을 거부하는 구현도 동일 정상 입력 대조에서 실패시킨다.
2. completion→EventNode→broker→worker의 책임 이전과 Full/Closed/중복/취소/결과 불명 전이를
   실제 소비 경로에 함께 연결한다. ACK 처리 뒤의 필수 효과도 확보하며 ID 여력을 byte 예산으로
   오독하지 않는다. 용량 통지는 등록→조건 재검사→대기의 lost-wake 검증과 함께 연결한다.
3. remote `transport.rs::serve`의 Full→연결 종료와 공용 outbound pump의 목적지 간 대기도 같은
   경계에서 다룬다. local actor GREEN을 remote 진행 증명으로 승격하지 않는다. grant를 envelope에
   싣는 설계라면 wire 버전·producer/consumer·구 peer 거부와 llama를 모르는 중립 responder가 필요하다.
4. 수정이 포화 형성 자체를 예방하면 시험 설치 과정에 정상 진행 분기를 둔다. 제출14개·결과6개·
   native KV/해제·순서 oracle는 유지하며 잘못된 포화 상태를 강제로 만들도록 구현을 왜곡하지 않는다.

세 라운드 제한을 새 slice 이름으로 초기화하지 않는다. 요청 이후 앞 기록의 추가 실행12가 첫
라운드이고, 이 고정 반례/정상 대조 및 전체 회귀 묶음13은 둘째로 기록한다. 마지막 라운드를
설계가 덜 정해진 후보 탐색에 쓰지 않는다. 기존 국소 수정으로 전체 순환 문제가 닫힌다고 한
전제가 성립하지 않으면 재설계 필요라고 보고하며, 세 번 안에 전체 B1~B8 완료를 보장하지 않는다.

### 2026-09-07 후속 구현 — 로컬 저장소 예약 기반과 실행 전 체크포인트

수정 전 RED는 `393a6c23e`에 별도로 보존했다. 이번에는 실행 결과를 보고 구현을 고르는 대신
원본 Event/공간 claim의 reserve→publish→owned dequeue→transfer/retire 전이를 먼저 고정했다.
실제 completion 저장소를 같은 원장에 연결하고, 보존 비용은 모든 소유 String/Vec의 capacity로 센다.
기존 일반 publication도 새 저장소를 사용한다. **운영 저장소 코드가 변경된 미검증 WIP**이며,
예약 경로의 제품 활성화·end-to-end 교착 수리는 아니다. 세부 API 제약은 배치 계약이 단독 소유한다.

코드 정적 검토에서 영구적인 단일 Event 초과를 Full 재시도로 취급하는 안과, RELEASE 미리보기 ID를
나중에 다시 발급하는 안을 배제했다. 전자는 기다려도 수용 불가능하고 후자는 앞선 효과의 ID 소비와
충돌한다. 생산자만 예약 API로 바꾸거나 여러 결과 전체를 cap1 슬롯에 예약하는 안도 정상 진행을
막는다. 이 판정을 확인하려고 추가 시험 실행을 하지 않았다.

**마지막 실행 라운드는 아직 사용하지 않았다.** 새 비용/저장소/실제 publication 거부/RELEASE oracle를
작성했지만 컴파일·시험·변이는 미실행이다. 이전1254/1/7은 수정 전 소스의 결과로만 남긴다.
기존 mailbox 시험과 actor cap1/cap8 입력·oracle는 변경하지 않았다. 자세한 변경·미실행 목록은
[정산 증거의 로컬 저장소 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)을 따른다.

**B1/B2/B5 IN_PROGRESS, B3 end-to-end 미완**. 다음 첫 행동은 새 API를 무조건 켜는 것이 아니라,
원인 작업의 후속 effect 보존과 수신 측 실제 공간 사이의 예약/책임 이전을 연결하는 것이다.

1. 실제 FIFO outbox에서 Event를 한 번 생성하고 ID 의무를 실제 번호로 전환한다. 예약된 저장소
   생산·owned 소비를 EventNode/broker와 함께 이관한다. RELEASE의 알려진 응답 표현과 가변 native
   결과 상한을 구분한다. native 전체 결과 상한이 없는 현재 HELLO를 임의 배수 예산으로 대신하지 않는다.
2. 기존 cap1/14입력/6결과를 그대로 승인 기준으로 사용한다. 예방적 진행으로 포화 설치가 불필요해질
   경우에만 실제 진행 witness가 있는 분기를 추가한다. 정상 입력을 제출하지 않거나 새 예산 때문에
   영원히 거절하는 구현은 실패다. remote·같은 순서 영역·취소/종료도 기존 검증 범위에서 누락하지 않는다.
3. 후보와 반례·정상 대조·변이 목록을 봉인한 뒤 마지막 검증 묶음을 실행한다. 현재 부분 API만 시험해
   마지막 라운드를 소모하거나, 새 이름으로 라운드를 초기화하지 않는다. 미검증 변경도 전체 WIP
   체크포인트로 보존하되 완료/성능 개선으로 보고하지 않는다.

이번 체크포인트에는 모델/GPU/C++/원격 실행·push가 없다. 전체 비무시 변경을 포함하고 생성물은
기존 ignore 경로에 유지한다. VRAM-only→RAM 오프로딩 강한 웨이브의 최종 목적은 바뀌지 않는다.

### 2026-09-07 후속 구현 — 고정 송신물과 예약 연결 전 정적 검토

앞 저장소 WIP는 `7f402aba5`에 보존했다. 이번 변경은 committed effect를 실제 FIFO 선두에서
한 번만 Event로 만들고, 최종 publication 실패에도 그 Event와 후속 의무를 보존하는 단계다.
기존 Full 내부는 이미 같은 Event를 재시도했다. 이번 수정이 그 사실을 처음 구현한 것은 아니다.
broker Full도 원본 allocation을 반환하도록 실제 destination 슬롯 확보를 복사보다 앞에 둔다.
계약의 정확한 상태 구분은 [배치 계약](adapter-batching-layers.md), 시험/정적 검토는
[정산 증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)가 소유한다.

**컴파일·시험·변이 미실행 WIP**다. 작성한 시험8개와 기존 실패 표현 시험의 이관을 통과로 세지
않는다. 두 독립 정적 검토에서 ID 의무 보존과 head 권한 재검증을 확인했고, 시험 이관의 전체
telemetry/Envelope 대조 누락은 실행 전에 보강했다. 수정 전 actor RED의 입력·oracle는 그대로다.
이것은 공간 선예약이나 동기 worker의 양보 구현이 아니므로 actor 교착 해결을 주장하지 않는다.

**B1/B2/B5 IN_PROGRESS, B3 end-to-end 미완; 마지막 검증 라운드는 미사용**이다.
다음 첫 행동은 부분 API를 시험하는 것이 아니라 아래 수용/책임 이전 설계를 실제 연결하는 것이다.

1. 현재 동기 FIFO에서 반환/일반 입력이 각각 어느 저장소를 점유하는지 확인하고, 원인 작업의
   전체 필수 효과 보존 공간과 전달 슬롯을 분리한다. pure 후보 원장에는 복제 가능한 RAII 권한을
   넣지 않는다. opaque 의무 ID와 worker 소유의 선형 공간 권한을 연결한다.
2. 동일 `(source, correlation)`의 PHYSICAL와 뒤따르는 관측은 목적지가 달라도 순서를 유지한다.
   SESSION 응답을 먼저 빼는 특례나 목적지별 무조건 우회만으로 설계를 닫지 않는다. 실제 수신 측
   공간, ACK 후 필수 효과, remote Full 및 native 가변 결과 상한까지 소유/거부 전이를 정한다.
3. 원인 승인 전 필요한 공간이 없으면 정상 보류하며, 기존 작업의 예약된 반환은 진행해야 한다.
   모든 입력을 막는 구현도 기존14입력/6결과 대조에서 실패해야 한다. 구현은 일반 중립 전달 API와
   어댑터의 flight/KV 의미를 분리하고, 실제 EventNode/worker 소비까지 연결한다.
4. 완성된 후보와 고정 반례·정상 대조·변이 묶음을 봉인한 뒤 남은 한 라운드를 실행한다. 컴파일러나
   시험 출력을 다음 설계 선택의 근거로 반복 사용하지 않는다. 예상 밖 실패는 그대로 기록하며
   세 번 안에 전체 B1~B8 또는 예외 없는 전역 완성을 보장했다고 하지 않는다.

이번에도 전체 비무시 변경을 WIP 체크포인트에 포함하며 생성물은 기존 ignore 경로에 둔다.
GPU/모델/원격/C++/push는 실행하지 않았다. 최종 성과 승격 조건과 승인 자원 범위는 §1 그대로다.

### 2026-09-07 후속 구현 — 전달 슬롯과 필수 결과 보존 공간 분리

앞 고정 송신물 WIP는 `bcbadf101`에 보존했다. 이번에는 알려진 fan-out 결과를 보관할 원자 예약과
작은 전달 큐를 분리했다. 큐 한 칸에 결과 세 개의 슬롯을 동시에 요구하는 안은 정상 작업도 시작하지
못하므로 배제했다. 이는 시험 실패 뒤 상한을 늘린 튜닝이 아니라 보존 공간/전달 슬롯의 책임 분리다.
그룹 메타데이터 자체의 비용·경합 재검사·취소와 wake의 경계는 [배치 계약](adapter-batching-layers.md)이 소유한다.

**컴파일·실행시험·변이 미실행 WIP**다. 그룹 예약10개·queue/retained 분리6개의 회귀 oracle를
작성하고 정적 대조만 했다. 일반 publication도 변경됐으므로 무동작 리팩터라 하지 않는다.
기존 actor 반례의 입력·cap1/cap8·결과·native oracle는 그대로이고 end-to-end GREEN을 주장하지 않는다.
생산자만 예약을 켜면 raw 소비자가 reserved front를 읽지 못하는 경계도 여전히 남아 있다.

**B1/B2/B5 IN_PROGRESS, B3 end-to-end 미완; 마지막 검증 라운드는 미사용**이다.
다음 첫 행동은 별도 예약 API를 더 만드는 것이 아니라 **한 원인 작업의 생산→owned 소비→수신 측
책임 이전을 실제 EventNode/worker 경로에 연결하는 것**이다. 먼저 아래 세 연결 조건을 한 번에 닫는다.

1. 원인 ID/응답 슬롯으로 선확보한 의무를 FIFO에서 최종 Event ID/sequence에 한 번 결속한다.
   pure 후보/원장 복사에는 RAII claim을 넣지 않고 worker가 선형 claim을 소유한다. 같은 순서 영역의
   앞선 송신물도 보존하며 broker의 정확한 중복 원장 보관 비용을 독립적으로 센다.
2. PublicationBlocked/EffectsRunnable/Idle/NativeInProgress/Fenced/Closing을 구별하고
   input+capacity+shutdown을 함께 기다린다. Full을 단순 Ok로 바꾸고 기존 `receiver.recv()`로
   내려가면 공간이 돌아와도 영구 대기할 수 있다. ACK 정산과 새 decode 발행 자원 확보를 결합하지 않는다.
3. 알려진 SESSION/제어 응답과 가변 native 결과를 분리한다. 현재 HELLO의 row/sequence 한도와
   수신 frame 상한은 cut tensor count/shape/dtype/alias의 사전 byte 상한이 아니다. native 실행 뒤
   `output_desc.nbytes`로 할당하는 경로에 임의 배수 예산을 붙이지 않는다. 실제 결과 bound와
   remote grant/Full·취소·종료 수용 계약이 없으면 그 부분은 미완으로 남긴다.

이 연결을 끝내기 전 부분 API 시험으로 마지막 회차를 소비하지 않는다. 새 시험·정상 대조·제거 변이를
같은 후보 소스에 봉인하고 기존14입력/6결과/외부 dequeue0 진행을 판정한다. 준비 중 소스를 계속 바꾸며
GPU 수치나 시험 출력으로 설계를 고르는 방식으로 되돌아가지 않는다. 필요한 소스·시험·계약만 전체
WIP 체크포인트로 남기며 생성물은 ignore한다. 성능·다중 컴퓨터 실기 목표는 아직 완료되지 않았다.

### 2026-09-07 후속 구현 — 수용 연결 전 PREFILL 거부 원자성

앞 저장소 분리 WIP는 `d8fff7d27`에 보존했다. 실제 생산→소비 연결을 정적 추적하던 중 PREFILL이
context/incarnation/admission 검증보다 먼저 session key를 기억하는 전제를 발견했다. 그대로 포화 중
입력 처리에 연결하면 거부된 요청도 흔적을 남기므로, 새 예약 API 추가 대신 **실제 수용 함수의 첫
쓰기 경계를 뒤로 옮겼다**. 예약 연결은 아직 하지 않았다. 제한된 보장과 바뀐 오류 우선순위는
[배치 계약의 L2 절](adapter-batching-layers.md#l2-수용점유-admission)이 단독 소유한다.

`prefill_admission_tests.rs`의 실제 codec→handle/직접 PREFILL oracle7개를 작성했다. 거부 시 상태
보존, 진단1개, 원인 수정 뒤 재제출, 기존 pending 우선의 정상 FIFO를 함께 검사하도록 했다.
**컴파일·시험·변이 미실행 WIP**이며 실제 실패를 실행했다고 쓰지 않는다. 기존 actor cap1/cap8의
14입력/6결과/외부 dequeue0 판정은 변경하지 않았다. 마지막 실행 결과는 여전히 수정 전1254/1/7이다.

**B1/B2/B5 IN_PROGRESS, B3 end-to-end 미완; 검증2회 사용·마지막1회 미사용**이다. 다음 첫 행동은
이 전제 수정 위에서 **producer→owned EventNode→broker→receiver의 실제 책임 이전**을 한 묶음으로
연결하는 것이다. raw 호환 다리로 회계를 끊거나 source claim을 exact dedupe 원장에 묶지 않는다.
blocked worker의 input/capacity/shutdown 대기, 원인별 필수 결과·반환 공간, native 결과 bound와
remote acceptance까지의 기존 미완 표면은 그대로다. 국소 PREFILL 수정으로 전체 교착을 닫았다고 하지 않는다.

코드 경로·소유자·오류 종류의 정적 대조를 먼저 끝내고, 완성 후보/반례/정상 대조/제거 변이를 봉인한 뒤
마지막 회차를 사용한다. 새 손잡이·큐 증설·실패 입력 축소로 진행 조건을 바꾸지 않는다. 유지할 소스·
시험·소유 문서만 전체 미검증 체크포인트에 포함하고 생성물은 ignore한다. GPU/원격/모델/C++/push는
이번 작업에서 실행하지 않았으며 최종 강한 웨이브 성과는 미완이다.

### 2026-09-07 후속 구현 — 실제 전달 거부의 원본 소유권

앞 PREFILL WIP는 `2b1d1d539`에 보존했다. 이번 연결 감사는 raw/owned 경계를 실제 호출자 전체로
추적했다. 중간에서 Event를 복사하고 claim을 버리는 opt-in 다리와, source claim을 exact dedupe
원장 수명에 묶는 안을 배제했다. 성공 경로 전체의 소유형 이관은 아직 하지 않았다.

실제 먼저 바꾼 것은 **canonical 거부 반환과 그 소비자**다. broker가 모든 거부의 원 Event를 반환하고,
adapter 입력 Closed도 원본을 돌려주며, EventNode terminal은 양방향 보류물을 반환한다. 제품 node
task가 그 결과를 보존하게 연결했다. 정확한 수명/미완 범위는
[중립 event 계약](event-protocol-v2.md#local-refusal-ownership--limited-implementation-boundary)이 소유한다.
이것은 raw Event 보존이지 공간 claim 이관이나 actor 교착 GREEN이 아니다.

broker4개·실제 node loop3개·llama try_offer2개 회귀를 작성했고 기존 API 시험을 새 반환값에 맞춰
원인/원문 대조를 유지했다. 원본 actor14입력/6결과/cap1·cap8/timeout/oracle는 그대로다.
**컴파일·시험·변이 미실행 WIP; B1/B2/B5 IN_PROGRESS, B3 end-to-end 미완**이다.
마지막 실제 결과1254/1/7은 수정 전 봉인 소스만 증명하며, 검증2회 사용·마지막1회 미사용이다.

다음 첫 행동은 이제 실패 원본을 반환하는 이 실제 경계에서 canonical 소유형 성공 경로를 이관하는
것이다. producer/held input·output/destination/WorkerInput/장기 request 원문을 함께 잇고, 정확한
중복 사본의 독립 비용 및 잠금 밖 알림을 유지한다. raw fallback이나 `handle(event.clone()); retire()`는
장기 보관 사본을 무과금으로 남겨 승인하지 않는다. 그다음이 아니라 **같은 후보의 승격 조건으로**
원인 작업의 필수 결과/반환 공간과 input/capacity/shutdown pump를 연결해야 cap1 진행을 판정한다.
native 결과 사전 bound·remote grant/acceptance·정상 prompt 웨이브는 계속 미완이다.

유지할 운영 변경·필수 회귀·소유 문서만 전체 미검증 체크포인트에 포함하고 생성물은 ignore한다.
이번에 remote/GPU/native/C++/push 실행은 없고, 새 한도나 정상 입력을 줄이는 정책을 추가하지 않았다.

### 2026-09-07 후속 구현 — 직접 응답 FIFO와 알림 경계

앞 거부 소유권 WIP는 `658c9cded`다. 이번에는 성공 전달의 원본/중복 사본을 분리하고, 실제 completion
enqueue 뒤 알림을 분리했으며, LOAD/SESSION/UNLOAD/오류 응답도 기존 committed FIFO에 연결했다.
**canonical owned 성공 전달 전체를 이관한 것은 아니다.** 실제 broker는 아직 raw 큐를 사용한다.

정적 대조에서 기존 native 오류1개 발행을 잃지 않는 좁은 종료 진단 경계와, batch 첫 Closed 뒤
나머지 진단 보존을 함께 검사했다. 최대 미래 ID 폭의 사전 검증은 경계 입력의 수용 범위를 보수적으로
줄이는 계약 변경이다. 기존 malformed ERROR 발행을 성공으로 세던 시험은 같은 입력으로 무발행·
무권한 승인·진단 보존을 요구하도록 고쳤다. 계약/필수 연결 범위는 [배치 계약](adapter-batching-layers.md),
oracle와 한계는 [정산 증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)가 소유한다.

**B1/B2/B5 IN_PROGRESS, B3 end-to-end 미완; 컴파일·시험·변이 미실행 WIP**다.
검증2회 사용·마지막1회 미사용이며 기존1254/1/7은 수정 전 결과다. actor14입력/6결과/cap1·cap8은
변경하지 않았다. 국소 API·직접 응답 보존으로 actor GREEN이나 성능 개선을 주장하지 않는다.

다음 첫 행동은 **배치 계약의 필수 연결 표를 실제 owned 소비로 닫는 것**이다. 새 보조 API만 추가하는
단계를 반복하지 않는다. `EventSender/Receiver`와 제품 저장소 생성, EventNode/NodeAdapter/WorkerInput,
장기 request/해제 출처, control/connection terminal 소비를 같은 변경 범위로 추적한다. source claim과
독립 dedupe 비용을 분리하고, 잠금 밖 알림·필수 결과/반환 선예약·input/capacity/shutdown pump를 함께
완료해야 고정 actor 반례의 진행을 판정한다. native 가변 결과 bound·remote acceptance는 별도 미완이며
raw fallback이나 row 임의 배수로 숨기지 않는다. 부분 컴파일/시험으로 마지막 회차를 소비하지 않는다.

이번 소스·회귀·소유 문서는 전체 WIP 체크포인트로 보존하며 생성물은 ignore한다. 원격/GPU/C++/push는
실행하지 않았다. 최종 강한 웨이브·정상 응답·VRAM-only 이후 RAM 오프로딩 승격 목표는 그대로다.

### 2026-09-07 후속 구현 — 실제 요청 입력의 불변 공유

앞 직접 응답 WIP는 `f5aa09675`다. canonical owned 연결을 정적 추적하며, raw 입력을 handler와
후보/배치 오류 보고가 다시 복제하는 경로를 확인했다. 이번 변경은 **이 반복 복사의 제거에 한정**한다.
실제 handler는 원 Event를 대여하고, RequestState 후보와 drive의 관측/오류 소유자는 불변 입력을
공유한다. 원본 요청의 출력 권한과 거부 시 진행 상태 보존은 바꾸지 않는다. 계약은
[배치 계약의 입력 공유 절](adapter-batching-layers.md#불변-수용-입력과-가변-진행-후보)이 소유한다.

PREFILL 수용의 원 Event 복사1회는 남아 있다. claim 이관·파싱 비용 예산·전체 메모리 상한 구현은
아니며, 앞 기록이 지시한 **owned 성공 연결 전체는 이번에도 미완**이다. 이 불일치를 숨기지 않는다.
새 ResourceBudget 숫자/기본값을 큐 개수로 만들어 넣지 않았다. 현재 제품 선언의 두 count 필드만으로
byte 한도를 증명할 수 없고, 알려진 응답과 가변 native 결과의 경계를 계속 구분한다.

**B1/B2/B5 IN_PROGRESS, B3 end-to-end 미완; 컴파일·시험·변이 미실행 WIP**다. 실제 prepare/accept
거부·정상 경로 및 공유 입력 수명 oracle3개를 작성했다. 기존 fixture의 값·단언·입력 수는 유지했다.
마지막 실행1254/1/7은 수정 전 결과이며 검증2회 사용·마지막1회 미사용이다. actor14입력/6결과의
cap1/cap8/외부 dequeue0 판정도 그대로다. 정적 검수에서 차단점이 안 보인 것과 실행 통과는 다르다.

**다음 첫 행동의 범위를 더 늘리지 않는다.** 배치 계약의 필수 연결 표에 이미 있는 canonical
EventSender/Receiver→EventNode→NodeAdapter/WorkerInput→장기 소유자를 실제로 이관한다. 먼저
제품 구성의 전달 슬롯/보존 count/byte/독립 receipt 한도 선언과 미선언 처리 방침을 확정하고, 숫자를
임의 보정하지 않는다. 필수 효과 선예약·input/capacity/shutdown pump를 같은 후보의 승격 조건으로
유지한다. 미결 bound나 remote acceptance를 감춘 부분 성공 경로로 완료를 선언하지 않는다.

이 단계에서 추가 선행 리팩터/손잡이를 이어 붙이지 않는다. 기존 반례를 통과할 완성 후보와 원인별
제거 변이를 봉인할 때만 마지막 검증을 쓴다. 소스·필수 회귀·소유 문서만 전체 WIP 체크포인트로
보존하며 생성물은 ignore한다. 원격/GPU/모델/C++/push 실행과 최종 성과 승격은 없다.
