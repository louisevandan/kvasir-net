# HF 독립 저장소 이관

2026-09-14 HF를 P4 내부 adapter로 통합하고 빌드·모델·두 host 검증을 완료했다.
[완료 보고](../../tests/reports/migration/20260914_120000.md)에 결과·실패·source 결속을 기록했다.
원본 내용은 모두 백업으로 이동했다. 프로세스 점유와 삭제 정책으로 빈 원본 디렉터리만 남았다.
기준: P4 `5b540770d`, HF `bc656a2a09422e452a69735eb411c12899caa57e`.
128개 추적 파일의 원본 hash와 처리 위치는 [파일 대응표](files.json),
시험 계약은 [이관 계획](../../tests/plans/migration-20260914.md)을 따른다.

현재 source 소유자는 P4 Git이며 adapter crate는 P4 root lock으로 빌드한다.
최초 요구·미연결 계획은 [이전 인수인계](../history/initial/HANDOFF.md)를 포함한 history에 보존한다.
과거 보고의 source commit/hash/원문 경로는 당시 증거이며 현재 실행 경로가 아니다.

원본 전체 Git bundle와 source zip, 비추적 증거는 로컬 P4 `.cache/hf/migration/20260914/`에 보관한다.
새 모델 캐시는 `.cache/hf/models/`, 환경은 `.cache/hf/environments/qwen3_5_0_8b/`, 새 결과는 `target/hf/`다.
환경·모델·Git backup은 추적 source archive에 포함하지 않는다.
기존 GitHub remote는 보존하며 이관 작업은 push/remote 삭제를 수행하지 않는다.

원본 내용은 backup의 `retired-project/`, 보조 worktree는 `retired-repair/`에 보존했다.
독립 프로젝트로 개발하거나 실행하지 않는다. 기존 `F:/dev/p4hfadapter/.git`과 부모는 빈 디렉터리다.
실행 소스 기준은 `38c635053`; 이후 완료 보고 변경은 문서만 포함한다.
