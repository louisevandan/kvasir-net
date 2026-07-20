# macOS Apple Metal acceptance baseline

이 문서는 Apple Silicon Mac을 linkcpp의 Metal 노드로 사용할 때의 **머신별
인수 기준**이다. Linux CUDA나 Windows CUDA의 기준을 대체하지 않는다. 특히
macOS에서는 Docker 컨테이너가 Metal 장치에 직접 접근하지 못하므로, 허브는
Docker에서 실행하고 Metal worker와 proxy stage는 호스트의 네이티브
node-agent에서 실행한다.

## 적용 대상

| 항목 | 기준 |
| --- | --- |
| 호스트 | macOS / Apple Silicon (arm64) |
| 가속기 | Apple Metal의 통합 메모리 장치 1개 |
| 허브 | Docker Desktop의 `hub` 컨테이너, 기본 포트 `19000` |
| 네이티브 agent | LaunchAgent가 실행하는 `controller.nodeagent`, 기본 포트 `9101` |
| RPC worker | 호스트의 `ggml-rpc-server`, 기본 포트 `50052` |
| 슬롯 | Metal은 물리 장치 1개이므로 논리 Slot 1 하나만 사용 |

## 사전 조건

1. Docker Desktop을 실행하고, 빌드 전에 다음 자원을 확보한다.

   - CPU: 사용 가능한 코어 전부 또는 빌드에 충분한 코어
   - 메모리: 최소 16 GiB, 권장 24 GiB 이상
   - 디스크 여유: 40 GiB 이상

2. Xcode Command Line Tools가 설치되어 `clang++`를 사용할 수 있어야 한다.
   native agent 런처는 `cmake`/`ninja`가 없을 때 해당 Python venv에 설치한다.

3. 모델 디렉터리를 설정한다. 기본값은 다음과 같다.

   ```sh
   export LINKCPP_MODEL_DIR="$HOME/.lmstudio/models"
   ```

4. 모델을 로드하기 전에는 기존 controller 또는 stage를 unload한다. Apple
   Silicon은 RAM과 Metal 메모리를 공유하므로, 동시에 두 모델을 로드하면
   운영체제가 메모리 압박 또는 재시작을 유발할 수 있다.

## 빌드 및 기동 절차

### 1. 허브 Docker 이미지

기본 허브도 RPC와 paired proxy runtime을 함께 빌드해야 한다.

```sh
docker compose build --build-arg LINKCPP_BUILD_JOBS=4 hub
docker compose up -d --no-build --force-recreate hub
docker compose ps
curl -fsS http://127.0.0.1:19000/api/runtime
```

`LINKCPP_BUILD_JOBS`는 Docker VM 메모리에 맞춰 조정한다. 24 GiB VM에서는
`4`가 보수적인 기본값이다.

Docker Hub 메타데이터 요청이 멈춰 빌드가 시작되지 않는 경우, 이미 존재하는
허브 이미지를 일시적 build base로 지정할 수 있다.

```sh
docker compose build \
  --build-arg LINKCPP_BUILD_JOBS=4 \
  --build-arg LINKCPP_BUILDER_IMAGE=linkcpp:latest \
  --build-arg LINKCPP_RUNTIME_IMAGE=linkcpp:latest \
  hub
```

이는 registry pull 장애 우회용이며, 정상 네트워크에서는 별도 인수를 주지
않는다.

### 2. 네이티브 Metal agent

`scripts/run-node-agent.sh metal`은 RPC worker뿐 아니라 아래 paired proxy
실행 파일도 같은 release build ID로 만들고 export해야 한다.

```text
build-node-darwin-metal/bin/ggml-rpc-server
build-node-darwin-metal/apps/linkcpp-node/linkcpp-node
build-node-darwin-metal/apps/linkcpp-server/linkcpp-server
```

LaunchAgent를 사용하는 설치의 기본 확인 명령은 다음과 같다.

```sh
launchctl kickstart -k "gui/$(id -u)/com.linkcpp.metal-slot1"
curl -fsS http://127.0.0.1:9101/control/status
curl -fsS http://127.0.0.1:9101/control/proxy/runtime
```

빌드 캐시가 다른 checkout의 절대 경로를 가리키면 런처가 캐시를 폐기하고
재구성해야 한다. `CMakeCache.txt`의 source-directory 오류를 수동으로
무시하지 않는다.

## 필수 인수 검사

### A. 허브와 agent의 release/프로토콜 일치

```sh
curl -fsS http://127.0.0.1:19000/api/runtime
curl -fsS http://127.0.0.1:9101/control/status
```

판정:

- hub와 agent의 `unit_version`, `runtime_pack_version`이 동일하다.
- agent의 backend가 `metal`, 장치가 Apple GPU로 보고된다.
- agent의 RPC endpoint가 `50052`이고 hub 컨테이너에서
  `host.docker.internal:50052`로 연결 가능하다.

### B. paired proxy runtime 검증

```sh
docker compose exec -T hub sh -lc \
  '/app/bin/linkcpp-node --runtime-info && /app/bin/linkcpp-server --runtime-info'
curl -fsS http://127.0.0.1:9101/control/proxy/runtime
```

판정:

- hub와 agent 모두 `ring_proxy.available`가 `true`다.
- protocol은 `linkcpp-stage-v1`이며, adapter ABI는 hub와 agent의
  `--runtime-info` 출력에서 같은 값으로 확인한다.
- 두 위치의 `build_id`가 정확히 동일하다.
- `state_snapshot`과 `chunked_state`가 모두 `true`다.

### C. 안전한 native stage smoke test

실제 모델 경로와 작은 레이어 창을 사용해 C++ stage process가 Metal에서
기동되는지 확인한다. 모델 전체 분산 로드를 시작하는 검사가 아니며, 완료 후
반드시 stop한다.

```sh
BASE=http://127.0.0.1:9101/control/proxy
MODEL='Qwen/Qwen3-Embedding-8B-GGUF/Qwen3-Embedding-8B-Q4_K_M.gguf'

curl -fsS -X POST "$BASE/stage/start" \
  -H 'Content-Type: application/json' \
  --data "{\"model\":\"$MODEL\",\"layers\":[0,1],\"role\":\"first\",\"listen_port\":51052,\"next_endpoint\":\"127.0.0.1:51053\",\"gpu_layers\":1,\"ctx\":128,\"parallel\":1,\"ring_protocol\":\"linkcpp-stage-v1\",\"ring_adapter_abi\":<ADAPTER_ABI>,\"ring_build_id\":\"<BUILD_ID>\"}"

curl -fsS "$BASE/stage/status"
curl -fsS -X POST "$BASE/stage/stop" \
  -H 'Content-Type: application/json' \
  --data '{"reason":"baseline smoke test complete"}'
```

판정:

- start 응답에 `accepted: true`와 실행 PID가 있다.
- status에 `running: true`가 표시된다.
- 로그에 Metal library 초기화가 보이며 즉시 crash하지 않는다.
- stop 뒤에는 `running: false`, `desired_load: null`이다.

### D. 자원 해제 확인

stage stop, controller unload, 또는 unbind 뒤에 다음을 확인한다.

```sh
curl -fsS http://127.0.0.1:9101/control/status
ps aux | rg '[g]gml-rpc-server|[l]inkcpp-node|[l]inkcpp-server'
```

판정:

- 종료 대상의 `worker_running` 또는 stage 상태가 false다.
- 이전 모델의 `linkcpp-node`/`linkcpp-server` process가 남지 않는다.
- 다음 모델 로드 전에 Metal/RAM 사용량이 안정화된다.

## macOS 고유 실패 분석

| 증상 | 직접 원인 | 조치 |
| --- | --- | --- |
| proxy API는 200인데 `available: false` | proxy 실행 파일 또는 runtime pack 누락 | `linkcpp-node`와 `linkcpp-server`를 native/Docker 모두 빌드하고 runtime-info의 build ID를 비교한다. |
| CMake source-directory 오류 | native build cache가 다른 checkout의 절대 경로를 보존 | generated build directory를 재구성한다. 런처는 stale cache를 자동 폐기해야 한다. |
| C++ `<version>` 오류가 root `VERSION`을 가리킴 | 대소문자 비구분 macOS volume과 llama.cpp server target의 root include path 충돌 | 최상위 CMake에서 macOS server target의 불필요한 root include path를 제거한다. `VERSION` 파일을 삭제/이름변경하지 않는다. |
| stage 경로에 runtime root가 두 번 붙음 | 절대 `LINKCPP_NODE_BUILD_DIR`에 작업 경로를 다시 prefix | 런처가 absolute/relative build dir를 구분해 `LINKCPP_STAGE_BIN`, `LINKCPP_SERVER_BIN`을 export해야 한다. |
| Docker build가 registry metadata에서 멈춤 | Docker Desktop pull 경로 장애 | Docker VM 안의 HTTPS와 host HTTPS를 분리 검사하고, 필요 시 로컬 hub 이미지를 build base로 사용한다. |
| 시스템 메모리 압박 또는 재부팅 | 통합 메모리 예산을 초과한 동시 model/stage load | Slot 1 RAM budget을 보수적으로 잡고 unload/stop 완료를 확인한 뒤 다음 load를 시작한다. |

## 기준 결과 기록 — 2026-07-19

| 검사 | 결과 |
| --- | --- |
| 호스트 | Apple M4 Pro / darwin arm64 / 64 GiB 통합 메모리 |
| Docker Desktop | 14 CPU, 24 GiB VM 메모리 |
| release | `0.0.10` |
| hub proxy runtime | `available=true`, `linkcpp-ring-0.0.10` |
| native Metal proxy runtime | `available=true`, `linkcpp-ring-0.0.10` |
| paired binary runtime-info | protocol `linkcpp-stage-v1`, ABI `5`, snapshot/chunked state enabled |
| proxy automated checks | 35 passed |
| native stage smoke test | 8B GGUF, layers `0:1`, Metal initialization 확인, start/stop 성공 |

## 후속 변경 시 보고 형식

새로운 macOS runtime, planner, release, Docker, agent 변경 뒤에는 이 문서의
필수 인수 검사를 다시 수행하고 아래 항목을 변경 보고에 포함한다.

```text
Date / commit / VERSION:
Host and Docker CPU/RAM:
Hub runtime-info:
Native agent runtime-info:
Hub build ID / agent build ID:
Stage smoke model and layer window:
Start result / stop result:
Memory observation and cleanup confirmation:
Failures, root cause, and remediation:
```
