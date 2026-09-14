# P4 HF event 통합 v2

현재 계약. P4 `layers/adapters/hf`가 Rust bridge·모델별 Python·환경과 worker 배포물을 소유한다.
P4 root workspace의 optional agent feature는 `hf-transformers`다.

| 목적 | 파일 |
| --- | --- |
| 공개 Rust 경계 | [crate](../../adapter/README.md) |
| 시험 계약 | [계획](../../tests/plans/p4-integration-20260914.md) |
| 기존 실기 증거 | [역사 보고](../../tests/reports/p4-integration/20260914_023000.md) |
| P4 조립 | [통합 안내](../../../../../docs/hf-integration.md) |
| 이관 상태 | [이관](../migration/README.md) |

## 빌드와 실행

작업 디렉터리는 P4 root다. `--model-dir`와 Python interpreter는 실제 준비 위치를 지정한다.

```powershell
cargo build --locked -p p4-agent -p p4-event-drive --features hf-transformers
python layers/adapters/hf/scripts/deployment/worker/run.py target/hf/bundle-A --label A
.cache/hf/environments/qwen3_5_0_8b/Scripts/python.exe layers/adapters/hf/scripts/verification/event_qwen/run.py --host 127.0.0.1 --port 41980 --plan layers/adapters/hf/plans/qwen3_5_0_8b/balanced_two_gpu_fp32/plan.json --scenario layers/adapters/hf/scenarios/qwen3_5_0_8b/short/scenario.json --bundle target/hf/bundle-A/bundle.json --output target/hf/short
```

agent는 별도 프로세스로 `p4-agent 127.0.0.1:41980 tcp://127.0.0.1:41980`처럼 시작한다.
`--nodes`는 `{agent,node,generation}` 목록, `--deployments`는 같은 순서의 `{python,bundle,model_dir}` 원격 절대 경로 목록이다.
각 worker의 bundle bytes는 동일하다. `--agent-binary`는 소비 바이너리 hash를 기록한다.
`epoch_eight`와 `--blocks 3`은 같은 LOAD에서 ID8개를3 epoch 재사용한다. 재실행은 새 generation을 사용한다.
event_matrix는 각 case의 generation을 증가시키고 고정 정상응답 기대값을 검사한다.

## 전송과 권한

OUTER 모델 controller의 단일 `scheduling.drive`가 step membership/issue/position 및 token 선택을 소유한다.
독립 reference 경로와 event 경로가 이 스케줄러를 소비한다. event controller는 worker pipe를 열지 않는다.
head worker 결과는 Rust retained completion → P4 broker → 다음 node의 Rust → Python을 거친다.
각 LOAD에서 topology와 OUTER owner를 고정한다. generation/epoch/serial 및 앞 stage의 동일 job receipt chain을 검사한 뒤만 실행한다.
cache 조회와 unload는 OUTER가 각 stage에 보내며 cache는 serial을 소비하지 않는 읽기이다.
receipt는 신뢰된 P4 transport상의 정산 기록이며 암호학적 인증 또는 악성 worker 검증을 주장하지 않는다.

| 착수 시 독립 v1 | P4 소비 v2 |
| --- | --- |
| controller가 각 worker pipe를 직접 호출 | OUTER controller→head retained→P4 broker→각 stage→OUTER |
| ready/run_id | bundle/config/model/tokenizer/plan/stage/generation/dtype/operation identity |
| active+retired 누적 상한 | 같은 v1 상태를 drained epoch마다 새로 만들며 과거 epoch 거부 |
| state 위치 대조 | 전체 attention KV·conv·recurrent 원소/구성/shape/dtype 대조 |
| 로컬 Python 실행 | 외부 Rust supervisor, P4 생성/INSPECT/수명, 물리 host별 배포 |

IPC는 기존 v1 framing(magic+version+reserved+u64 BE length)에 v2 packet을 싣는다.
packet은 u32 BE JSON 길이, JSON object, opaque body다. JSON ≤64KiB, frame ≤32MiB이며 receipt도 frame에 포함한다.
LOAD identity는 protocol/bundle SHA256/config/model/tokenizer/plan/stage/generation/dtype/boundary/operations를 확인한다.
stdout은 framing 전용, stderr는 ≤64KiB로 별도 수집한다. worker 환경은 Qwen runtime_check와 함께 고정한다.

## 예산과 수명

CREATE는 bounded mailbox와 감독 thread만 만든다. LOAD만 worker를 실행한다.
입력에는 upstream 원본 claim을 유지하면서 별도 byte 상한을 계수한다. Full/Closed는 원본 allocation/claim을 반환한다.
출력은 worker 호출 전 최대 packet과 최대 경로 envelope를 예약한다. dequeue 이후 held claim도 count/bytes에 남는다.
IPC body 복사용 scratch는 최소 4×frame이다. 별도로 입력/출력 저장소, JSON ≤64KiB의 유한 metadata,
16개 topology, bounded stderr, bundle 검증 중 최대 16MiB 파일 버퍼를 계산해야 한다. frame 한도를 전체 heap 상한으로 읽지 않는다.
한 bridge의 실제 model command는 하나다. input queue와 output queue/retained 한도는 CREATE 명세다.

실행 중 EOF/잘린 frame/잘못된 identity/timeout은 uncertain으로 fence한다. 첫 입력 claim과 읽은 응답 bytes를
명시 abort까지 유지하고 같은 issue를 자동 재시도하지 않는다. façade는 살아 있어 회수 제어를 처리한다.
모델 오류와 cleanup 오류는 분리한다. timeout은 LOAD/command ≤120초, graceful exit 5초, 강제 회수 10초다.
`abort`는 LOAD generation 전체의 명시 포기이며 epoch/serial/issue/position=0, request=""여야 한다.
요청 cancel/release와 다르며 이전 LOAD generation은 거부한다. barrier 도중 일부 stage만 전환됐어도 각 worker를 회수할 수 있다.
UNLOAD/epoch는 이전 held output이 있으면 거부한다. 자신의 최종 ack도 회수되기 전 snapshot은 busy다.
DELETE는 기존 P4의 adapter 상태·권위 있는 completion storage·건강한 node task 검사를 그대로 소비한다.

v1 `active+retired<=8` 계약은 보존한다. v2는 모든 stage release를 확인한 OUTER가 명시 epoch barrier를 발행한다.
각 stage는 active=0에서만 새 bounded StageSessions를 만든다. 이전 epoch step/release/cancel은 실행 전에 거부한다.
한 stage라도 거부/불명인 barrier는 정상 진행으로 간주하지 않고 전체 LOAD를 abort한다.

## Python 교체와 source 출하

A를 UNLOAD/DELETE하고 새 B bundle로 다시 LOAD한다. 동일 agent hash에서 bundle hash/readiness label 변화와 정상 요청을 검증한다.
protocol/hash가 맞지 않는 bundle은 LOAD 전에 거부하며 실행 디렉터리를 덮어쓰지 않는다.

clean P4 commit에서 단일 source archive를 내보낸다. schema2는 P4 commit/archive SHA256/restore tool SHA256를 기록한다.
기존 schema1의 두 저장소 archive는 과거 bundle에 포함된 그 당시 restore.py로만 복원한다.

```powershell
python layers/adapters/hf/scripts/deployment/source/run.py export target/hf/source-bundle
python target/hf/source-bundle/restore.py restore target/hf/source-bundle target/hf/reproduced
```

standalone helper는 `reproduced/p4` 하나를 복원하고 root lock으로 agent/event-drive의 feature on/off를 빌드한다.
Git이나 sibling HF checkout은 필요 없다. Rust toolchain과 고정 의존 패키지는 필요하다.
Python 환경/weight는 source archive에 포함하지 않는다. worker bundle은 별도로 만든다.
Windows venv launcher는 실제 interpreter를 자식으로 만든다. CREATE_SUSPENDED로 시작한 launcher를
KILL_ON_JOB_CLOSE job에 넣은 뒤 main thread를 재개하고, abort ack 전 job의 active process=0을 확인한다.
프로세스 전역 검색으로 다른 worker를 종료하지 않는다. Job 객체의 하위 프로세스 상속은
[Microsoft 계약](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-assignprocesstojobobject)을 따른다.
갑작스런 host 전체 종료의 durable 복구는 이 in-memory 계약에 포함하지 않는다.
