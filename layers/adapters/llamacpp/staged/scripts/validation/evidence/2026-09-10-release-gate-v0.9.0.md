# 2026-09-10 — v0.9.0 초기 게이트와 마감 재검증

**현재 상태: 릴리즈 후보 보존 완료, 정식 봉인 BLOCKED.** 새 agent/drive의 r256 실행이 Windows Update 재시작으로 중단됐다.
로그인 복구 뒤 r256 재판정과 메모리 부족 거부 게이트가 남았다. 아래 최초 게이트와 최신 마감 기록을 구분한다.

종류: 릴리즈 트리에서 재빌드한 산출물의 실기 게이트. 성능 증거가 아니다.
기준 커밋 `890417a77`, 작업 트리 clean. 로드맵 P-3e 턴 1의 완료 조건을 이 문서가 판정한다.
장소는 `m42-server2`(RTX 3090 ×2), 로그온 상태, 실행 전 카드 313 MiB.

## 산출물 결속

빌드: prepared tree `0eadefebd3-f37f181c9d38`(26 patches, 0026 포함), Ninja **Release**, CUDA 13.1,
`86;89`. 첫 구성은 스크립트가 Ninja에 `--config`만 넘겨 **Debug**로 잡혔고(메모리에 기록된 함정),
중단 뒤 같은 cache를 `-DCMAKE_BUILD_TYPE=Release`로 재구성해 다시 빌드했다. CTest **15 / 15 통과**(3.13 s).
`p4_staged_server.exe`는 빌드 루트에 출력되고 `bin/`에는 DLL만 놓이므로 flat copy는 그 둘과 CUDA 런타임
`cublas64_13`·`cublasLt64_13`·`cudart64_13`을 모아 만들었다.

배포 전 원격 상태는 `staged.pre-v090-20260909T174456Z`·`p4-agent.exe.pre-v090-20260909T174456Z`로 보존했다
(이전 서버 `b66beffb…`, 이전 에이전트 `eaa153f4…`).

| 파일 | sha256 (로컬 = 원격, 배포 로그에서 복사) |
| --- | --- |
| `p4_staged_server.exe` | `fd06c12237083c5e247108e3b3704aa9d7b205c3ee022d6e812f05c408fd9487` |
| `llama.dll` | `90f21982c2e354c92ddda16edd1de4abe1d6445e529a1ad17b85df8201480a1b` |
| `ggml-cuda.dll` | `8a8ed0d4b3938cf634e01e10ff75a475355ed61f57f0a152492aa38eab896c97` |
| `ggml-base.dll` | `1e74e53afab3806f7a2826d81ed711103110b8c43e029e5cd3101616ca420eca` |
| `ggml.dll` | `ddd78d3dbf7afb76d2da625889773769300df905ae08e3ec608916e659378671` |
| `p4-agent.exe` | `cf1c7002a3c4e3c1ee40fe4330f00bd0a51f33bac9dc24bac591fd384be5388a` |
| `p4-event-drive.exe`(로컬 드라이브) | `3cdac71588c3ae0bb477c49b8617af2ac1b4ef8bd8cea2099ac41ac2df2969a4` |

에이전트 `cf1c7002…`는 09-09 배포본 `eaa153f4…`과 바이트가 다르다. 그 사이 에이전트에 들어가는 비시험
소스 변경은 `ownership.rs` 시험 모듈의 11줄뿐이며, 바이트 차이의 원인은 확정하지 않았다.
각 실행의 `evidence.json`이 원격 해시를 다시 기록하므로 아래 표의 판정은 그 값과 대조한다.

## 게이트

산출물은 `target/release-gate-v090/<실행 ID>/`이며, 숫자는 각 실행의 `artifact.json`·`evidence.json`·
`agent.stderr.log`에서 복사했다. 네 실행 모두 `evidence.json`이 원격 서버 `fd06c122…`, `llama.dll`
`90f21982…`, `ggml-cuda.dll` `8a8ed0d4…`, 에이전트 `cf1c7002…`, launcher `2cdfc22d…`(09-09와 동일)를
기록해 위 결속표와 일치한다. 커밋은 네 실행 모두 `890417a77`.

**작업 트리 기록에 대한 정정.** #1은 `working_tree_clean=true`이고 #2~#4는 `false`다. 게이트가 도는 동안
이 문서 자체(untracked, 4,134 B, sha256 `6f6204d4…`)와 `docs/document-map.md`의 등록 한 줄을 작업 트리에
썼기 때문이며, 각 실행의 `dirty.diff`가 그 두 파일만을 담고 있다. 소스는 바뀌지 않았다.
실행이 봉인한 것은 `890417a77`의 소스와 배포 해시이고, 이 문서는 그 결과를 적는 파일이다.

| # | 시나리오 | 실행 ID | req / completed / released | error / cleanup_error | 생성 TPS(참고) | 판정 |
| ---: | --- | --- | --- | --- | ---: | :-- |
| 1 | `smoke` (2B, 4 stage) | `20260909T174533Z-2a88df2b` | 1 / 1 / 1 | null / null | 17.77 | **통과** |
| 2 | `pressure` (2B, 4 stage, r256) | `20260909T174740Z-5b7f716f` | 512 / 512 / 512 | null / null | 400.19 | **통과** |
| 3 | `pressure_35b` (35B, 2 stage, r96) | `20260909T175343Z-5ca4c41f` | 512 / 512 / 512 | null / null | 278.27 | **통과** |
| 4 | `release_35b_r256` (35B, 2 stage, r256) — 0026 | `20260909T180528Z-f8972a0c` | 512 / 512 / 512 | null / null | 330.12 | **통과** — 아래 0026 근거 |
| 5 | `release_35b_r512_must_refuse` | `20260909T181615Z-55018c0d` | 적재 거부 | `n_seq_max must be <= 256` | — | **무효** — 계획에 닿기 전 llama.cpp 상한에서 거부됐으므로 계획의 거부 증거가 아니다 |
| 5′ | `release_35b_must_refuse` (r256, ctx 1024, ubatch 4096) — 0026 | `20260909T181846Z-6c01faf4` | 적재 거부 (artifact 없음, 추론 전 거부) | `staged memory plan exceeds currently free memory` | — | **통과** — `fits_current_free=false`로 거부. 아래 stage 구분 정정 참조 |

#3·#4의 응답은 직접 검사했다: `<think>` 유출 0/512, ChatML 마커 유출 0/512, 서로 다른 응답 375·373/512,
최다 반복 24자 창 0.06·0.11(마크다운 구조), 종료는 전부 `length`(고정 길이 부하시험).
최대 VRAM(창 내 `memory.used` 최대)은 #3 **15,619 MiB**, #4 **21,459 MiB**(24,576 중 87 %).

### 0026 판정 근거 (각 실행 `agent.stderr.log`의 계획 기록, `plan-lines.mjs`)

| 실행 | n_seq | planning pass `CUDA0 RS buffer` | `free` | `context` | `required` | `fits_current_free` |
| --- | ---: | ---: | ---: | ---: | ---: | :-- |
| 09-09 `…e6c5c4c4` (0025 트리) | 256 | 7,504 MiB | 15.43 | 7.99 | 19.72 | false |
| #3 (0026 트리) | 96 | **0.00 MiB** | 22.76 | 3.00 | 14.01 | true |
| #4 (0026 트리) | 256 | **0.00 MiB** | **22.76** | **7.99** | **19.72** | **true** |
| 5′ head `MEMORY_PLAN` | 256 · ctx 1024 · ubatch 4096 | **0.00 MiB** | 22.76 | 8.66 | 22.64 | **true** (여유 0.12) |
| 5′ head `MEMORY_ACTUAL` | 같음 | 7,504 MiB (실제) | 0.11 | 8.66 | **22.64** | 할당 크기는 계획과 일치 |
| 5′ tail `MEMORY_PLAN` | 같음 | **0.00 MiB** | 22.76 | 8.66 | **23.64** | **false → 실제 적재 전 거부** |

#3·#4 모두 계획 pass(plan #1)의 `CPU RS buffer`·`CUDA0 RS buffer`가 **0.00 MiB**로 찍히고, 그다음의
실제 pass(`MEMORY_ACTUAL`, record #2)에서 09-09와 같은 2,814 MiB·7,504 MiB가 실제로 잡힌다. `context`는 0025 때와 같은
3.00·7.99 GiB로 보고되어 **정렬을 포함한 예상 크기 보고가 유지**됐고, `free`는 22.76 GiB로 카드 초기
여유 그대로다. 09-09에 `required 19.72 > free 15.43`으로 거부됐던 #4 구성이 같은 `required 19.72`에
`free 22.76`으로 **admitted**됐고, 그 뒤 실제 적재·512건 추론·해제·UNLOAD까지 끝났다.

통과 조건 판정. #4: 계획 pass RS **0.00 MiB** ✓, `context` 예상 크기 유지 ✓, `fits_current_free=true` ✓,
실제 적재·추론·UNLOAD ✓, 최대 VRAM 21,459 MiB 기록 ✓.
5′: **거부됐다** — 서버가 `staged memory plan exceeds currently free memory`를 내고 stage가 exit 5로 끝나
드라이브가 적재 단계에서 실패했다(추론 전이므로 `artifact.json` 없음, 보존 계약대로). fit 검사
`required() <= free`는 그대로 살아 있고 넘치는 구성을 실제로 막는다. r512는 계획에 닿기 전
`n_seq_max must be <= 256`에서 거부돼 이 목적에 무효였다.

**마감 감사 정정 — 과소 보고 주장을 철회한다.** 이전 기록은 head의 `MEMORY_PLAN`과 tail의
`MEMORY_PLAN`을 같은 stage의 계획/실제 대조로 오독했다. 원문 순서는 head PLAN → head ACTUAL →
tail PLAN이다. head의 model/context/compute는 각각 11,276,284,416 / 9,294,577,664 /
3,742,433,280 B로 계획과 실제가 정확히 같고, tail의 별도 계획은 11,923,591,680 /
9,294,577,664 / 4,169,138,176 B다. tail은 `required 25,387,307,520 > free 24,436,015,104 B`로
실제 적재 전에 거부됐다. 이 실행에는 tail의 `MEMORY_ACTUAL`이 없다.

`load.rs`는 같은 agent의 stage를 순서대로 적재한다. `llama_stage_runtime.cpp`는 계획을
`MEMORY_PLAN`, 실제 할당을 `MEMORY_ACTUAL`로 출력하고, 실제 대조에서는 free를 다시 fit 검사하지
않고 model/context/compute의 일치 여부를 검사한다. 할당 뒤의 free 감소는 과소 보고 증거가 아니다.
따라서 model 0.60 GiB·compute 0.39 GiB의 "편차"는 서로 다른 stage의 비용 차이이며 알려진 결함에서
삭제한다. #3·#4도 각 stage의 PLAN/ACTUAL을 짝지어 대조해야 한다.

분석 도구가 ACTUAL을 건너뛰면서 그 앞 RS 실제 할당 로그를 다음 PLAN에 붙이던 것을 고쳤다.
`plan-lines.test.mjs`는 실제 CLI에서 head PLAN/ACTUAL/tail PLAN, ACTUAL 단독, 손상된 ACTUAL을
검사한다. 수정 전 3 실패 → 수정 후 3 통과, 별도 복사본에서 ACTUAL 처리를 제거하면 3 실패다.
Node 실행 파일과 각 해석 소스의 SHA256, 원본 실패·통과 로그는 마감 원자료에 보존한다.

## 마감 재검증 — 소스 3302591fc, 정식 봉인 BLOCKED

기능·시험 수정 커밋 `3302591fc7e2a69298cfa0b2e9c9378629cf293b`, 최종 네 시도 모두 clean 트리다.
이후 후보 보존 커밋은 문서·manifest와 해시 파일 줄바꿈 규칙만 바꾼다. 기존 미공개 v0.9.0 tag는 archive ref로 보존하며,
새 바이너리의 필수 게이트가 남은 상태에서 정식 tag로 재봉인하지 않는다. push는 하지 않았다.

### 완료한 수정·검증

- `plan-lines.mjs`: PLAN/ACTUAL 종류를 그대로 기록하고 각 경계에서 버퍼 로그를 비운다.
  실제 head RS를 다음 tail PLAN에 붙이던 오류를 고쳤다. 실제 CLI 회귀 3개: 수정 전 3 실패 → 수정 후 3 통과,
  독립 복사본에서 ACTUAL 처리를 제거하면 3 실패.
- `build-stage-server.mjs`: configure에 `CMAKE_BUILD_TYPE`을 전달하고 cublas/cublasLt/cudart를 실제 서버 exe 옆에 복사한다.
  실제 builder를 실행하고 외부 process/filesystem 경계만 대체하는 회귀 4개를 추가했다.
  독립 변이: configure 연결 제거 2 실패, 복사 위치를 Release 폴더로 고정 1 실패.
  모든 해석 소스 및 Node exe SHA256과 실패/성공 로그를 보존했다.
- 공식 builder를 실제 CUDA Release cache에서 재실행: Release configure, Ninja `no work to do`, **CTest 15/15**,
  runtime copy 완료. native 소스와 compat 26개 패치는 초기 CUDA 빌드 뒤 바뀌지 않았다.
- 버전 0.9.0의 Rust agent/drive는 별도 `CARGO_TARGET_DIR`에서 `cargo build --release --locked
  -p p4-agent -p p4-event-drive`로 재빌드했다. 새 agent를 원격에 배포하고 새 drive로 아래 실기를 실행했다.
- 전체 workspace 최종 실행: **1,374 passed / 0 failed / 7 ignored / 0 filtered**.
  첫 실행은 README 혼합 줄바꿈 때문에 docs_lint 1개 실패했고, CRLF 수정 후 전체 재실행이 통과했다.
  `workspace.log`와 `workspace-final.log` 모두 보존했다.
- Node 전체 **146 passed / 0 failed / 0 skipped**. 기존 보고와 같은 범위는 94→101(+7개 회귀),
  나머지 45개는 compat/upstream 검증기 시험을 이번 집계에 추가한 것이다.
  docs-lint **91 files clean**, compat manifest·patch classification **26 valid**, private headers **81 clean**.

| 파일 | 실제 배포/실행 SHA256 |
| --- | --- |
| `ggml-cuda.dll` | `8a8ed0d4b3938cf634e01e10ff75a475355ed61f57f0a152492aa38eab896c97` |
| `llama.dll` | `90f21982c2e354c92ddda16edd1de4abe1d6445e529a1ad17b85df8201480a1b` |
| `p4_staged_server.exe` | `fd06c12237083c5e247108e3b3704aa9d7b205c3ee022d6e812f05c408fd9487` |
| `p4-agent.exe` | `4316965abe718bf2feb6b895586824f3b51a74512ae69110699a2e4724d9a543` |
| `p4-event-drive.exe` | `44cc922603ddd1b5e4874590cd2bcb6b595d70a9b90c6e456197ec2c8cda8bf3` |

권위 필드는 원격 이미지 `evidence.remote.images`, 로컬 drive `evidence.binaries.drive_sha256`다.
generic `binaries.server_sha256`·`ggml_cuda_sha256`는 기존 로컬 cache b66beffb/3452e117 값이므로 원격 실행 이미지로 읽지 않는다.
`deployment.json`의 staged 파일/DLL 10개와 agent/drive 해시, 각 실행의 실제 원격 이미지·소스·clean 상태 및
원본 MANIFEST를 `audit-runs.mjs`로 다시 대조했다.

### 최신 실행 결과

| 시나리오 | 실행 ID | req/completed/released | 판정 |
| --- | --- | --- | --- |
| smoke | `20260909T190252Z-b9e2cef9` | 1/1/1 | PASS, error/cleanup_error null |
| pressure (2B r256) | `20260909T190458Z-bfde8e1d` | 512/512/512 | PASS, error/cleanup_error null |
| pressure_35b (r96) | `20260909T191117Z-853157c9` | 512/512/512 | PASS, error/cleanup_error null |
| release_35b_r256 | `20260909T192319Z-f82b070a` | 512/0/0 | OS 계획 재시작으로 중단된 실패. 재판정 필요 |
| release_35b_must_refuse | 새 바이너리로 미실행 | — | BLOCKED, 로그인 복구 필요 |

최신 r96은 102,400 생성 token / 385.142 s = 265.88 raw TPS이며, 전체 GPU 캡처 최대 `memory.used`는
15,646 MiB다. 수치의 분모는 `artifact.elapsed_ms`, 분자는 OUTPUT token 수이며 품질 통과 TPS/개선율이 아니다.
smoke는 400 token length 종료 1건, 두 pressure는 200 token length 종료 각 512건이다.
초기 통과·최신 통과·최신 중단의 할당 완료 stage 모두 같은 stage의 PLAN/ACTUAL을 짝지어 host/device
model/context/compute를 바이트 단위로 대조했고 일치한다. 이것은 해당 구성의 점별 대조이며 전체 모델/backend 무회귀는 아니다.

### r256 중단의 원인과 부분 결과

실행은 512개 delivered, completed/released 0, **승인된 OUTPUT token 16,896개**를 부분 artifact로 보존했다.
최초 추론 오류는 stage 연결의 os error 10054이고 cleanup_error도 별도로 남았다.
`evidence_missing={requests:512,stage_executions:2}` 때문에 요청별 행 수는 0이며 출력 token 증거와 혼동하지 않는다.
두 stage 모두 적재는 끝났고 PLAN/ACTUAL 값이 일치했다. 중단 실행의 전체 캡처 최대 VRAM은 21,486 MiB다.
이 실행의 부분 TPS를 처리량 승인에 쓰지 않는다.

Windows System 이벤트 1074 원문:

- **19:29:29.082 UTC**: `MoUsoCoreWorker.exe`, 계획된 OS 서비스 팩 재시작(0x80020010), SYSTEM.
- 19:29:29.582 UTC: nvlddmkm 이벤트 153. 이를 먼저 GPU 결함의 원인으로 단정하지 않는다.
- **19:32:21.868 UTC**: `TrustedInstaller.exe`, 계획된 OS 업그레이드 재시작(0x80020003).
- 19:32:56 UTC 이후 boot, SSH 복구. 로그온 사용자 0, agent/stage process 0을 확인했다.

따라서 이 시도는 **Windows Update 계획 재시작으로 중단된 실패**로 분류한다. 통과로 집계하거나 삭제하지 않는다.
`r256-restart-initiator.json`·`r256-driver-events.json`·`r256-host-restart.json`에 XML·시각을 보존했다.
최초 조사에서 WER가 과거 dump를 재보고한 항목은 이번 BlueScreen의 증거로 쓰지 않는다.
마감용 일회성 wrapper가 `evidence.json` 부재를 읽은 ENOENT는 후속 오류이고, 최초 오류는 보존된 artifact에 있다.

### 후보 보관과 재개

- 원자료 **10개 실행**(초기 6, 마감 4)을 보존한다. 무효 r512와 OS 재시작 중단도 포함한다.
  Git `bundles/v0.9.0/`의 10개 MANIFEST 사본에는 75개 파일 해시가 있고,
  `sha256(SHA256SUMS)=e57b49a793512aba19328b0c6876687221def3fe8096cef09c58b6d1d932d959`다.
- 후보 runtime/source/evidence ZIP과 `RELEASE-STATUS.json`·`SHA256SUMS`를
  `F:/dev/p4-releases/v0.9.0/` 및 원격 `C:/Users/42mob/p4-remote/releases/v0.9.0/`에 보관한다.
  runtime의 실제 import에 필요한 Microsoft VC++ x64 runtime/Windows UCRT와 NVIDIA driver는 환경 의존성이며,
  CUDA DLL·의존 라이선스·내부 검증 스크립트는 포함한다. 모델 가중치는 포함하지 않는다.
- 기존 tag 객체는 `refs/archive/v0.9.0-pre-close-20260910` 및 원자료에 보존했다.
  후보 archive의 소스 commit은 외부 상태 JSON이 결속하며 **정식 릴리즈 tag/push는 보류**한다.
- **재개의 첫 행동:** 42mob 대화형 Windows 로그인과 S: 접근 확인 → OS/driver/파일 해시 확인 →
  동일 binary/시나리오로 r256 재판정 및 must_refuse 실행 → 새 결과를 현재 실패와 함께 기록 → 최종 annotated v0.9.0.
  Windows Update 정책·드라이버·로그인 설정은 변경하지 않았다.

H0~H7·다중 물리 호스트·정상 완결 응답·서비스 승인·TPS 개선은 미달성이다.
다음 버전 개발은 no-alloc 나머지 모델/backend 행렬 → B2/B3 → 정상 응답/원인 trace → H5 순서로 이관한다.
