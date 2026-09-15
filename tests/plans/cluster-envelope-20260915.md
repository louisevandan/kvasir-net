# 모델 없는 클러스터 반환 경로 검증 계획

2026-09-15. 사용자 지정 범위: 모든 보유 클러스터의 사양·노드 목록 조회와 MI250 SSH 중계.
계약은 [명시적 반환 문맥](../../docs/event-protocol-v2.md#required-request-return-context)이 소유한다.

## 환경과 전제

- Windows 로컬·m42·TUF, Linux Spark·Ubuntu, mac20·mac21, 외부 MI250 두 대: 물리 9대.
- `dde0813fb` 소스와 동일한 시험용 agent. 기존 설치 앱과 다른 디렉터리·포트를 사용한다.
- 모델을 생성/로드하지 않는다. 시험 agent의 nodes는 처음부터 끝까지 빈 목록이어야 한다.
- LAN은 TCP41997. MI250은 Ubuntu SSH jump를 거쳐 로컬이 유지하는 정·역방향 loopback 터널을 사용한다.
- SSH 자격 증명은 로컬 기존 키를 사용하며 키 내용은 복사하거나 증거에 넣지 않는다.

## 절차와 기대값

1. 기존 설치 주소와 SSH 접근을 따로 조사한다. 특정 포트 실패를 설치 앱 전체 장애로 간주하지 않는다.
2. Git archive와 플랫폼별 바이너리 SHA256을 고정한다. POSIX는 `cargo build --release --locked -p p4-agent --features hf-transformers`로 빌드한다.
3. [실행기](../../tools/cluster-envelope-check.py)에 `{name, host, port, address}` 배열을 전달한다. dial 주소와 P4 advertised 주소를 구분한다.
4. 직접 조회, 모든 접수×대상 조합 3회, 대상 목록 3배의 교차 요청, 같은 채널의 세대1/2 동시 요청, 세대3 재연결을 검사한다.
5. 모든 응답의 target/return_route가 원 OUTER와 같고 source는 조회 대상이어야 한다. correlation/causation과 사양·빈 노드 목록도 대조한다. 이벤트 ID는 서로 다른 의뢰에 재사용하지 않는다.
6. absent/mismatch 반환 문맥을 실제 TCP에 넣어 EOF/reset 거부를 확인하고 정상 조회를 다시 수행한다. 시간 초과는 거부 성공이 아니다.
7. LAN 7×7과 로컬 gateway/MI-A/MI-B 3×3을 각각 실행한다. 이는 9×9 단일 mesh 시험이 아니다.
8. 결과와 실패 원본, stderr, source/binary hash를 보존한다. 전체 시험 뒤 직접 조회로 nodes=[]를 확인하고 소유 PID·task·방화벽 규칙·SSH 터널만 정리한다.

## 증거와 범위

명령: `python tools/cluster-envelope-check.py CONFIG OUTPUT`.
원자료: `target/cluster-envelope-20260915/`. 커밋되는 요약 봉인과 실행 결과는 보고서에 연결한다.
모델 없는 조회는 기본 event runtime 반환을 검증한다. 노드 파생 이벤트·모델별 혼합 owner·생성/취소의 재검증이나 대형 모델 성능 수용으로 확대하지 않는다.
연결 단절의 불명 결과 정산·재접속 정책과 기존 FINISH 실패는 별도 잔여 항목이다.
