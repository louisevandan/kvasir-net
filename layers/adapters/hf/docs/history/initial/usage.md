> 역사 기록: 독립 저장소 시점의 요구·상태·측정이다. 현재 배치와 사용법은 [HF 안내](../../../README.md)를 따른다. 원본 전체는 이관 시 보존한 Git bundle에 있다.

> 2026-09-14: 사용자 §0 지시로 P4 통합 구현을 진행한다. 이전 미연결/읽기 전용 설명의 현재 상태는 [통합 명세](../../integration/README.md)와 그 수용 보고가 우선한다.

# 현재 사용과 예정 운영 흐름

지위: 현재 가능한 명령과 미래 흐름을 구분합니다.

## 지금 가능한 것

Qwen3.5-0.8B의 모델 준비·계획 검사·시나리오 실행은 [전용 스크립트 안내](../../models/qwen3_5_0_8b/README.md)를 따릅니다.

```powershell
Set-Location F:\dev\p4hfadapter
Get-Content AGENTS.md
Get-Content HANDOFF.md
git status --short
git log -1 --oneline
git remote -v
python -B tools/testing/run.py
python -B tools/verification/framing_mutation/run.py
python -B tools/verification/documents/run.py
```

위 Python 명령은 표준 라이브러리만 사용하며 설치된 CPython 3.13.15/Windows에서 검증했습니다.
framing 변이 명령은 `artifacts/framing/`의 새 폴더에 독립 복사본·전체 로그·SHA256을 기록하고 원본을 수정하지 않습니다.
전송 시험의 peer는 검증용 프로세스이며 모델 worker가 아닙니다.
`pip install -e .`, `cargo build`, P4 서비스와 quantize 명령은 아직 없습니다. Qwen 실행은 위 전용 진입점을 사용합니다.
P4 문서는 읽기 전용으로 참고하며 기존 P4의 build/deploy 명령을 이 프로젝트 실행 명령처럼 사용하지 않습니다.

## 향후 운영 순서

1. 고정된 모델/장치/양자화 manifest와 버전 lock을 검증합니다.
2. 원본 또는 공개 양자화 가중치를 확보하고 필요한 경우 별도 준비 작업으로 양자화합니다.
3. stage별 필요한 tensor와 metadata를 패키징하고 해시를 검증합니다.
4. OUTER가 노드를 구성하고 각 stage의 LOAD를 요청합니다.
5. worker의 실제 커널·메모리·capability를 확인한 뒤 session 경로를 바인딩합니다.
6. 요청을 수용해 prefill/decode·정산·출력·release를 진행합니다.
7. 종료 때 새 수용 중지 → 실행 정지/정산 → 상태/예약 반환 → unload를 확인합니다.
8. 실패 때 정상 종료와 구분한 부분 결과·최초 오류·정리 오류·잔존 자원을 보존합니다.

위 흐름의 CLI/REST/socket 문법은 아직 미정입니다. 웹서비스를 붙여도 P4 입장에서 OUTER 클라이언트이며,
모델별 실행 의미를 P4 공통 코어로 옮기지 않습니다.

## 환경과 산출물

Python 환경은 이 저장소의 `.venv/` 등 독립 위치에 만듭니다. P4의 환경/빌드 디렉터리를 재사용하지 않습니다.
CUDA/ROCm/MPS에 필요한 패키지·커널·compiler는 지원표 확정 후 설치합니다.
Python lock과 Rust Cargo.lock은 실제 패키지 생성 시 정책을 정해 재현 가능한 조합으로 관리합니다.

가중치는 루트 `models/`, 측정은 `artifacts/`, Rust 출력은 `target/`, 로컬 감사는 `.local/` 같은 무시 경로를 사용합니다.
`python/p4hfadapter/models/<model>/<role>/`의 전용 모델 소스는 Git 추적 대상입니다.
실제 대용량 파일은 외부 경로에 둘 수 있으나 manifest에 digest와 재확보 방법을 남깁니다.
credentials와 모델 가중치는 Git에 넣지 않습니다. 원격 URL·배포 호스트·access token은 아직 설정하지 않았습니다.

## 향후 통합 배포

P4 바이너리에 Rust bridge를 링크하는 것과 Python 환경·모델·커널을 배포하는 것은 별도 산출물입니다.
양쪽 revision과 worker package identity를 묶어 배포하고 불일치 시 LOAD를 거부하는 계약을 구현합니다.
최종 P4 통합 작업의 변경 위치와 범위는 [아키텍처](architecture.md)를 따릅니다.
