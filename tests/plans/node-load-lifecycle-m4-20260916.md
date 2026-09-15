# LOAD·UNLOAD 노드 수명 M4 결정론적 실행계획

## 봉인 범위

기준은 `c55c9ce8f24bf17ab461e7da7dae9f4f8dbe8b8e`다. M4는 M3 최종 소스로 실제 작은
llama.cpp와 HF 모델을 LOAD하고 정상 생성·요청 교차·취소·UNLOAD·빈 INSPECT·새 generation
재LOAD를 검증한다. 실행 뒤 수명 계약 소유 문서를 현재 경로로 이관한다. 장문 8wave 성능과
Qwen3.5-122B-A10B Release A 승격은 다음 로드맵 단계이며 이 수용 수치에 합산하지 않는다.

## 사전 판정과 실행 자원

- 이 Windows PC에서는 build와 model run을 하지 않는다. 모델 파일의 경로·크기·hash 확인과 문서 검사는
  저부하 작업으로만 수행한다.
- Linux Spark `m42@192.168.0.26`은 20 logical CPU 중 Cargo jobs/test threads 최대 8을 쓴다.
  Windows `M42-SERVER2` `42mob@192.168.0.29`는 28 logical CPU 중 Cargo jobs 최대 8을 쓴다.
- Spark의 기존 PID `1287453`은 프로세스만 남고 `:52005` listener가 없어 양쪽 TCP INSPECT가 timeout이다.
  이 작업 소유가 아니므로 종료하거나 정상 agent/빈 node로 판정하지 않는다. 새 작업 소유 agent만 별도
  포트에서 시작하고 PID·시작 시각·실행 파일·listener를 봉인한다.
- M42-SERVER2의 시작 상태는 `p4-agent.exe` 0개, agent listener 0개, RTX 3090 두 장 모두
  26 MiB 사용·0%다. HF와 Windows llama.cpp는 `CUDA_VISIBLE_DEVICES=0` 또는 native `CUDA0`만 쓴다.
- GGUF는 `Qwen3.5-0.8B-Q8_0.gguf`, 833,592,736 byte,
  SHA-256 `c54f8b67069c70085b98440de696b44da8250250ac69a961b41133def876e262`다.
  HF checkpoint revision은 `2fc06364715b967f1860aea9cf38778875588b17`, safetensors는
  1,746,942,600 byte, SHA-256 `04b1c301231dd422b8860db31311ab2721511346a32cb1e079c4c4e5f1fe4696`다.
- 기존 Linux/Windows native server는 P4 source `bfc8bcc45c4d2f8bd8723add6c8f1190db623989`라서
  현재 READY resource profile보다 오래됐다. 봉인 모델 memory-plan에서 필수 상한 세 값이 누락됨을 확인했으므로
  이를 재사용하지 않는다. 최종 M4 source에서 Windows CUDA 12.8·compute 86·parallel 8로 다시 build·test하고,
  새 binary/DLL hash와 양의 resource profile을 확인한 뒤에만 LOAD를 허용한다.
- 포트는 각 OS의 실제 dynamic range와 기존 listener를 읽은 뒤 고른다. 작업 소유 방화벽 규칙과
  프로세스만 종료하며 최종 listener 0을 확인한다.

## 과거 교훈 재사용

- `L001`~`L040`을 전부 적용한다. 특히 L002/L003의 실제 route preflight가 통과하기 전 모델 LOAD를
  시작하지 않고, L021/L028에 따라 Linux·Windows 원격 다단계 명령은 전송한 파일만 실행한다.
- 최신 commit은 Git bundle로 두 host에 전달한다. L010의 commit object 확인, clean checkout,
  Cargo 절대 경로와 jobs 8을 확인한 뒤 build한다. 실행 source와 binary hash를 결과에 결속한다.
- 모델·Python·bundle·native DLL/SO를 모두 hash/버전으로 검사한다. missing dependency를 실행 실패로
  발견하지 않고 preflight에서 차단한다.
- 각 LOAD는 시작 전 `nodes=[]`를 요구한다. 성공·거부·불명 결과의 causation과 typed lifecycle metadata를
  대조하고, UNLOAD terminal을 받은 뒤 별도 INSPECT로 `nodes=[]`를 확인한다.
- 정상 생성 기대값, 취소 시점, request 수, GPU 선택을 실패 뒤 완화하지 않는다. 기존 historical
  CREATE/DELETE 기록은 고치지 않되 현재 실행 trace에서 그 content type이 0인지 검사한다.

## 봉인 시험

| ID | 실제 경로 | 판정 |
| --- | --- | --- |
| M4-P0 | source·binary·model·Python·GPU·port·agent/node preflight | 두 새 agent의 정확한 왕복, nodes 0, task-owned child/listener 0, hash/버전 일치 |
| M4-L1 | Spark OUTER→Spark/Windows agent→실제 llama.cpp 2stage | CREATE/DELETE 0, LOAD만으로 두 node 생성, 정확한 `4`/EOS/release, UNLOAD 뒤 양쪽 nodes 0 |
| M4-L2 | 같은 node ID의 새 generation으로 llama.cpp 2요청 재적재 | 서로 다른 정상 prompt/응답, 요청별 완료·release, 과거 generation 효과 0, 최종 nodes 0 |
| M4-L3/NL12 | 첫 실제 llama stage LOAD 뒤 둘째 stage를 고정된 잘못된 binary로 거부 | 첫 node를 OUTER가 UNLOAD, P4의 타 node 자동 회수 0, first/cleanup 오류 구분, 양쪽 nodes 0 |
| M4-H1 | M42-SERVER2 최신 feature-on agent와 실제 HF single GPU short | `4`·`서울`, reference logits/cache 일치, UNLOAD 뒤 nodes 0와 worker 0 |
| M4-H2 | 새 generation HF interleaved cancel | 정상 두 요청의 reference 일치, 취소 요청은 `cancelled_at_step_boundary`, release/cleanup 후 nodes 0 |
| M4-H3 | 다시 새 generation HF short | 새 worker PID/LOAD identity, 같은 정상 응답과 parity, 이전 generation 거부, 최종 nodes 0 |
| M4-R | 표적·Python 전체·workspace feature off/on·문서 검사 | failed 0, ignored 별도 집계, 최종 source와 검증 source 동일 |

M4-L1/L2는 두 물리 agent와 동일 patch-set의 플랫폼별 native를 사용한다. 플랫폼 호환 readiness가 다르면
설정이나 기대값을 완화하지 않고 원문을 보존한다. HF는 RTX 3090 CUDA0 하나만 보이게 하며 다른 GPU와
로컬 desktop GPU는 사용하지 않는다.

## 라운드와 실패 처리

라운드 1 전에 M4-P0, 모든 runner의 static parse, config validator, 모델 기대값 postprocessor를 통과시킨다.
첫 실행은 L3의 의도된 거부와 L1/L2/H1~H3 전체를 한 봉인 묶음으로 수행한다. 예상 밖 실패 시 같은 명령을
재시도하지 않는다. 원본 로그와 source/binary/input identity를 보존하고 장부에 새 교훈과 자동 차단을 추가한
뒤 단일 원인만 고친다. 라운드 2는 그 미세 조정, 라운드 3은 기능 변경 없는 확인과 독립 변이다.

3회 안의 성공을 목표로 한다. 3회 뒤에도 실패하면 반증된 설계를 그대로 반복하지 않고 원인·상태 전이를
다시 닫아 라운드 4를 연다. 라운드 5는 그 재설계의 확인에만 쓴다. 5회 뒤에는 완료로 표시하지 않는다.

## 완료 산출물

- 실행 원문과 요약은 `tests/reports/node-load-lifecycle/<timestamp>.md` 및 remote raw artifact에 남긴다.
- `event-protocol-v2.md`, `layer-isolation-contract.md`, `distributed-batching-verification.md`,
  `adapter-batching-layers.md`, HF/실행 README를 새 LOAD/UNLOAD 소유권으로 갱신한다.
- README·문서 안내도·로드맵·수명 계획에 보고를 연결하고 Qwen122B 다음 첫 행동과 미수용 H0~H7을 적는다.
- 모든 비무시 변경을 한 복원 가능한 커밋으로 만들고 push한 뒤 local/remote HEAD와 dirty 0을 확인한다.
