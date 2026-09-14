# HF 독립 저장소 이관

2026-09-14 사용자 승인으로 HF를 P4 내부 adapter로 통합한다. 검증 중이며 삭제 완료는 아직 아니다.
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
