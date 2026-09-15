> 현재 실행 위치: P4 root. 아래 모델 세부 계약은 유지하며 현재 명령은 [사용법](../../usage.md)을 따른다. 이 문서의 HF 상대 경로를 사용할 때는 `layers/adapters/hf`에서 실행한다. 환경/모델 캐시는 P4 root `.cache/hf`에 있다.

> v1 독립 controller의 설명이다. 새 v2 P4 경로는 [통합 명세](../../integration/README.md)를 따른다.

# Qwen3.5-0.8B 노드 분할 실행

<a id="automatic-loading-planner"></a>

## 자동 로딩 계획

`planning.plan_loading`과 CLI `plan`은 이 모델의 실제 stage 프로필을 받아 기존 v1 plan JSON을 생성한다.
모델 형식은 safetensors이며 고정 revision·dense FP32·CPU/CUDA만 자동 계획한다.
GGUF 파싱, Mac/GB10 tier 우선순위, 모든 레이어에 동일한 KV 공식을 적용하지 않는다.

요청 예시는 [loading_cpu/request.json](../../../plans/qwen3_5_0_8b/loading_cpu/request.json)이다.
예시의 16 GiB는 시험용 예산이다. 실제 가용량을 조사한 값으로 바꾸고 OS/allocator/미관측 peak reserve를 남긴다.

| 입력 | 계약 |
| --- | --- |
| `model_id`, `revision`, `dtype`, `quantization`, `limits` | 현재 모델 실행 계약과 일치. limits는 context·max_requests·max_new_tokens |
| `prefill_chunk` | 측정 최대 chunk. 생성 plan의 `limits.prefill_chunk`로 전달해 scenario/worker에서 실행 전 거부 |
| `cuts` | 0과24를 포함하는 오름차순 경계. 모든 앞/뒤 경계 조합을 device별로 측정해야 함 |
| `hosts` | host ID, available_bytes, reserve_bytes. 여러 stage의 프로세스 RAM을 합산 |
| `devices` | ID, host, cpu/cuda:N, available_bytes, reserve_bytes, enabled. 한 물리 host/device는 한 ID |
| `slots` | node_id/device_id의 순서. 생략 가능한 stage 자리이며 같은 device를 여러 번 참조 가능 |
| `minimum_hosts` | 선택할 host ID 수의 하한. ID/프로필만으로 물리 머신 수를 인증하지 않음 |

P4 root에서 새 출력 폴더를 사용한다.

```powershell
.cache/hf/environments/qwen3_5_0_8b/Scripts/python.exe -B layers/adapters/hf/scripts/models/qwen3_5_0_8b/cli/run.py profile --request layers/adapters/hf/plans/qwen3_5_0_8b/loading_cpu/request.json --output layers/adapters/hf/target/loading-profile/new-run --timeout 600
python -B layers/adapters/hf/scripts/models/qwen3_5_0_8b/cli/run.py plan --request layers/adapters/hf/plans/qwen3_5_0_8b/loading_cpu/request.json --profiles layers/adapters/hf/target/loading-profile/new-run/profiles.json --output layers/adapters/hf/target/loading-plan/new-run
python -B layers/adapters/hf/scripts/models/qwen3_5_0_8b/cli/run.py inspect --plan layers/adapters/hf/target/loading-plan/new-run/plan.json
```

`profile --host <id>`는 현재 로컬 머신을 해당 ID에 명시적으로 대응시킨다. 원격 접속은 하지 않는다.
각 머신에서 만든 profiles.json을 `plan --profiles <file1> <file2>`로 합친다. 모델 경로는 `--model-dir`로 지정할 수 있다.
`plan`은 checkpoint의 대형 tensor나 torch를 읽지 않고 프로필의 고정 모델·source·runtime·workload 결속을 확인한다.
누락/오래된 프로필은 용량 부족과 구별해 거부한다. 모델/worker 변경 후 프로필을 다시 측정한다.

각 후보를 새 Python 프로세스에서 `load_stage`로 적재한다. safetensors index가 선택한 실제 weight byte,
전체 context까지 동시에 살아 있는 max_requests개의 KV/conv/recurrent cache, 프로세스 peak RSS와
CUDA allocator peak reserved byte를 측정한다. 끝에 실제 release로 모든 active state를 회수한다.
첫/마지막 stage의 tied embedding 복제와 단일 stage의 alias는 loader가 실행한 양 그대로 반영한다.

현재 HF controller는 stage 반환을 순서대로 기다린다. 따라서 목적은 **전체 stage service 합 → 최대 stage service → stage 수**다.
동일 GPU의 여러 stage는 device 용량을, 같은 host의 여러 GPU/CPU stage는 host RAM을 누적한다.
Pareto 탐색은 주어진 순서의 slot 부분집합과 cuts 안에서 정확하다. 상태 수 상한을 넘으면 미판정 오류이며
용량 부족으로 바꾸지 않는다. 후보 밖의 cut/device 순서·IPC/network 비용·공유 GPU contention은 최적성 주장 범위 밖이다.
측정은 synthetic tokens/hidden states와 cold stage 실행이며 실제 TPS/품질/모든 입력의 메모리 상한을 보증하지 않는다.

출력은 `plan.json`, `assessment.json`, `request.json`, `profiles.json`이다. 기존 `run`/`verify`와 P4 event
pipeline이 plan을 그대로 소비한다. standalone 실행은 `host=local`; P4 원격 배포 주소와 bundle은 기존 별도 명세다.
계획·시험·프로필 생성기·검증 runner와 이 기능의 ignored `target/` 결과는 모두 HF 서브폴더에 있다.

2026-09-15 로컬 검증: Python42/42, 기존 TS34/34, 독립 전수 탐색180개 사례, 제거 변이7/7을 확인했다.
CPU 실측3개 구간에서 split tied weight 추가량1,017,118,720 bytes와 live cache44,433,408 bytes를 확인했다.
생성된2-stage CPU plan으로 공식 모델과6스텝 logits/greedy 비교를 통과했고 두 응답은 `4`, `서울`이며 둘 다 EOS다.
초기 검증 runner의 빈 set을 빈 dict와 비교한 assertion 실패는 `real-cpu/`에 보존하고, 타입 비교를 고친 새 실행은
`layers/adapters/hf/target/loading-planner-20260915/real-cpu-final/summary.json`에 기록했다. 모델 기대값/허용 오차는 바꾸지 않았다.
oversized chunk의 실제 StageSessions 거부는 forward·active·retired 효과0을 확인했다. 다중 물리 host 수용·BF16 해결·H0–H7 승격은 아니다.
같은 생성 plan을 작업 소유 localhost P4 agent의 NODE_LOAD→HF worker→release/NODE_UNLOAD로도 소비했다.
`target/loading-planner-20260915/p4-event/summary.json`은 logits/greedy6회·cache12회 비교와 agent 종료를 기록한다.
HF 활성 `cargo test --locked --workspace --no-fail-fast --features hf-transformers`는 별도
`--target-dir layers/adapters/hf/target/loading-planner-20260915/cargo`에서1460 passed/0 failed/7 ignored,
61 summaries와exit0이다. 기본 target의 첫 시도는 사용 중인 `target/debug/p4-agent.exe` 제거가 거부돼
build exit101이었으며 실행 중인 agent를 중단하지 않았다. 로그는 `workspace-hf.log`와 `workspace-isolated.log`로 구분한다.

선정 모델은 `Qwen/Qwen3.5-0.8B`, revision `2fc06364715b967f1860aea9cf38778875588b17`입니다.
텍스트 입력의 prefill·decode를 **노드별 실제 로컬 Python 프로세스**로 실행합니다.
각 노드는 지정 레이어의 weight와 요청별 cache만 소유합니다. 노드 간 전달은 독립 시험 controller의 bounded IPC입니다.
P4 연결, 원격 호스트 실행, 이미지/영상, MTP, 양자화, 물리 continuous batching은 포함하지 않습니다.

## 실행

저장소 루트 PowerShell에서 실행합니다. 검증 환경은 Windows/CPython 3.13.15,
PyTorch `2.14.0+cu130`, Transformers `5.17.0`, safetensors `0.8.0`입니다.
runtime 패키지 목록은 [환경 lock](../../../environments/qwen3_5_0_8b/requirements.lock)에 고정했습니다.

```powershell
uv venv ../../../.cache/hf/environments/qwen3_5_0_8b --python 3.13.15
uv pip install --python ../../../.cache/hf/environments/qwen3_5_0_8b/Scripts/python.exe -r environments/qwen3_5_0_8b/requirements.lock --index https://download.pytorch.org/whl/cu130 --default-index https://pypi.org/simple
../../../.cache/hf/environments/qwen3_5_0_8b/Scripts/python.exe -B scripts/models/qwen3_5_0_8b/preparation/run.py

python -B scripts/models/qwen3_5_0_8b/cli/run.py inspect --plan plans/qwen3_5_0_8b/balanced_two_gpu_fp32/plan.json
../../../.cache/hf/environments/qwen3_5_0_8b/Scripts/python.exe -B scripts/models/qwen3_5_0_8b/cli/run.py run --plan plans/qwen3_5_0_8b/balanced_two_gpu_fp32/plan.json --scenario scenarios/qwen3_5_0_8b/short/scenario.json --output ../../../target/hf/qwen/my-run
../../../.cache/hf/environments/qwen3_5_0_8b/Scripts/python.exe -B scripts/models/qwen3_5_0_8b/cli/run.py verify --plan plans/qwen3_5_0_8b/balanced_two_gpu_fp32/plan.json --scenario scenarios/qwen3_5_0_8b/interleaved_cancel/scenario.json --output ../../../target/hf/qwen/my-verify
```

`inspect`는 모델 설치·다운로드·GPU 접근 없이 계획을 검사합니다. `run`은 생성을 수행하며 `verify`는
동일 입력 token을 공식 전체 모델에도 공급해 매 스텝 logits와 greedy token을 비교합니다.
`--output`은 새 폴더여야 하며 기존 결과를 덮어쓰지 않습니다. 이미 확보한 checkpoint는 `--model-dir`로 지정합니다.
실행 전에 [봉인 hash](../../../manifests/qwen3_5_0_8b/artifact/identity.json)와 전체 파일을 대조합니다.

## 노드 계획

`layers: [start, end]`는 0부터 시작하는 **끝 제외 구간**입니다. 순서대로 `[0, 24)`를 빈틈·중복 없이 덮어야 합니다.
`node_id`는 고유하고 `device`는 `cpu` 또는 `cuda:N`입니다. 첫 stage가 embedding, 마지막 stage가 norm/head를 소유합니다.
head의 tied weight는 원본 embedding tensor에서 가져오며 서로 다른 stage이면 명시적으로 복제·용량 계산합니다.
실행은 `host: "local"`만 허용합니다. 다른 host 값의 배치 계획은 `inspect`까지 가능하며 원격 실행으로 가장하지 않습니다.

| 계획 디렉터리 | 분할 | 정밀도·장치 |
| --- | --- | --- |
| `single_gpu/` | 0–24 | BF16, cuda:0 |
| `balanced_two_gpu_fp32/` | 0–12 / 12–24 | FP32, cuda:0 / cuda:1 |
| `uneven_three_stage_fp32/` | 0–5 / 5–19 / 19–24 | FP32, cuda:0 / cuda:1 / cuda:0 |
| `attention_boundaries_fp32/` | 0–3 / 3–4 / 4–24 | FP32, DeltaNet 전용·attention 전용 stage 포함 |
| `cpu_gpu/` | 0–4 / 4–24 | FP32, CPU / cuda:0 |
| `single_cpu/` | 0–24 | FP32, CPU |

모든 계획 파일은 `plans/qwen3_5_0_8b/<이름>/plan.json`입니다.
`balanced_two_gpu/`, `uneven_three_stage/`, `attention_boundaries/`에는 BF16 실험 계획도 보존합니다.
**RTX 4080 + 3090의 BF16 균등 분할은 단일 4080 기준 대비 logits 허용값을 초과했습니다.**
이 BF16 조합을 검증 통과 구성으로 취급하지 않습니다. 같은 4080 내 BF16 분할은 오차 0이었고,
FP32는 별도 recipe/실행 결과입니다. BF16 실패의 기준·입력·정밀도를 사후 변경해 통과로 재분류하지 않았습니다.

계획 상한은 context ≤4096, 실행 요청 수 ≤16입니다. 제공 예제는 context 2048·요청 8·출력 64 이내로 고정했습니다.
상한은 모델 공식 최대 context나 전체 GPU 메모리 수용 보장이 아닙니다. CUDA 번호는 PyTorch 열거 순서이며
이 장치에서는 cuda:0=RTX 4080, cuda:1=RTX 3090으로 관측했습니다.

## 시나리오

| 시나리오 디렉터리 | 처리 |
| --- | --- |
| `short/` | 산술·한국어 요청을 순서대로 생성하고 각각 해제 후 노드 재사용 |
| `chunked_prefill/` | 긴 입력을 32 token씩 prefill한 뒤 반복 decode |
| `interleaved_cancel/` | 서로 다른 길이·chunk의 요청을 round robin으로 교대, 하나는 2 token 뒤 취소·해제 |

파일은 `scenarios/qwen3_5_0_8b/<이름>/scenario.json`입니다. 요청마다 prompt, max_new_tokens,
prefill_chunk, cancel_after를 지정합니다. 취소는 스텝 경계에서 일어나며 실행 중 CUDA kernel의 즉시 중단을 뜻하지 않습니다.
round robin은 여러 독립 요청의 상태를 교대 소비하는 방식입니다. 물리 연산 배치는 항상 요청 하나이며 padding을 사용하지 않습니다.
EOS·출력 상한·취소를 구별하고 완료 뒤 모든 stage에서 해당 상태의 제거를 확인합니다.

## 모델별 구현과 감사

공식 config의 텍스트부는 24층, hidden 1024이며 3개의 Gated DeltaNet과 1개의 full attention이 반복됩니다.
출력 head는 embedding과 tied입니다. 제조사 구조의 근거는
[고정 config](https://huggingface.co/Qwen/Qwen3.5-0.8B/blob/2fc06364715b967f1860aea9cf38778875588b17/config.json)와
[모델 카드](https://huggingface.co/Qwen/Qwen3.5-0.8B)입니다.

실제 호출·cache API는 [Transformers v5.17.0 구현](https://github.com/huggingface/transformers/blob/595ff117c8412ec01262084058276e6b27a857d9/src/transformers/models/qwen3_5/modeling_qwen3_5.py)과
[cache 코드](https://github.com/huggingface/transformers/blob/595ff117c8412ec01262084058276e6b27a857d9/src/transformers/cache_utils.py)에 결속했습니다.
`forward/`는 이 공식 모듈을 호출하며 범용 layer introspection을 하지 않습니다.

| 전용 역할 폴더 | 소유 |
| --- | --- |
| `identity/`, `configuration/` | 각각 고정 모델 identity와 노드 계획 검증 |
| `preparation/`, `evidence/` | 각각 checkpoint 확보와 파일·소스·실행 hash 검증 |
| `loading/` | meta 모듈 생성 후 index로 담당 weight만 선택 적재 |
| `forward/` | 구상 Qwen layer의 부분 forward; 전역 weight 이름과 로컬 cache index 분리 |
| `state/` | stage-local DynamicCache, 요청별 position/issue, 거절·해제·늦은 요청 방지 |
| `boundary/` | JSON metadata + safetensors tensor payload; pickle 사용 없음 |
| `worker/` | 한 노드의 로컬 프로세스 진입점 |
| `processes/`, `routing/` | 각각 자식 프로세스/IPC 수명과 승인된 스텝의 노드 순회 |
| `scenarios/`, `execution/` | 각각 요청 admission과 prefill/decode 스케줄 실행 |
| `reference/`, `cli/` | 각각 공식 전체 모델 비교와 사용자 명령 진입점 |

부분 적재 시 다른 레이어나 vision 모듈을 먼저 할당하지 않습니다. 각 node report에 실제 tensor 이름·byte를 남깁니다.
cache는 해당 stage 레이어 수만 갖고, full attention이 없는 stage는 KV 길이 추정 대신 명시적 position을 사용합니다.
마지막 stage만 마지막 입력 위치의 logits를 반환합니다. 앞선 prefill 위치의 전체 logits를 IPC로 보내지 않습니다.

## 검증과 한계

```powershell
python -B scripts/testing/run.py
../../../.cache/hf/environments/qwen3_5_0_8b/Scripts/python.exe -B scripts/verification/qwen_state/run.py
../../../.cache/hf/environments/qwen3_5_0_8b/Scripts/python.exe -B scripts/verification/qwen3_5_0_8b/run.py
../../../.cache/hf/environments/qwen3_5_0_8b/Scripts/python.exe -B scripts/verification/qwen_mutation/run.py
```

고정 판정은 logits `atol=0.125, rtol=0.01`과 모든 비교 스텝의 greedy token 일치입니다.
이는 선택 시나리오의 실행 정합성 검사이며 일반 품질/SLO 인증은 아닙니다. 세부 증거는
[시험 계획](../../../tests/plans/qwen3_5_0_8b-20260913.md)과 [실행 보고](../../../tests/reports/qwen3_5_0_8b/20260913_220709.md)를 따릅니다.

attention은 eager, DeltaNet/causal convolution은 설치된 PyTorch fallback입니다. 압축 커널·최적화 커널의
성능을 주장하지 않습니다. 요청 수·context 초과, 미지원 모델/양자화, 중복 issue, 잘못된 tensor는 거부합니다.
worker 오류/timeout은 재시도하지 않고 해당 실행을 실패 처리하며 자신이 시작한 자식만 정리합니다.
복구 가능한 분산 원장이나 durable exactly-once, authenticated remote service는 미구현입니다.

결과 폴더는 고정 plan/scenario, `summary.json`, node별 stderr를 포함합니다.
summary에는 출력 전문·token·종료 이유·release, 스텝별 상태 byte/위치, 실제 PID/장치,
checkpoint/source/upstream 코드 hash·package 버전과 최초 오류·정리 오류가 있습니다. 성능 수치는 일반 벤치마크로 해석하지 않습니다.
