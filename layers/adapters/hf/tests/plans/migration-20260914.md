# Test Plan: HF repository migration

Created: 2026-09-14. P4 base `5b540770d`, HF source `bc656a2`.

## Goal and environment

Windows/PowerShell/Rust/Python3.13에서 단일 P4 checkout의 빌드·fixture·실제 모델 동작·복원을 검증한다.
원본 Git과 비추적 자산의 사전 backup을 검증하고 마지막에만 원본을 제거한다.
별도 P4 worktree와 새 출력 디렉터리를 사용한다. model revision/lock/기대값/상한은 기존 계약 그대로다.

## Steps and expected results

1. MIG-RED: 이관 전 P4 source를 HF sibling 없이 복원. Cargo metadata exit101과 missing dependency를 보존한다.
2. MIG-GRAPH: 단일 workspace에 HF가 포함되고 P4 neutral package identity 각각1개. sibling 의존 제거 변이는 실패한다.
3. MIG-UNIT: Python suite, HF retained fixture, cache/epoch 및 독립 Rust/Python 변이. fixture 부재는 성공이 아니다.
4. MIG-ALL: 최종 입력을 봉인하고 P4 전체 workspace on/off 종료·모든 summary·ignored를 집계한다.
5. MIG-RUNTIME: 새 agent에서12 fixture 시나리오, Qwen local matrix/epoch/교체/회수와 llama on/off 정상 생성.
6. MIG-DIST: 새 worker bundle과 agent로 두 실제 host의 Qwen matrix·반환 단절 후 abort/재수용. 다른 작업의 자원은 종료하지 않는다.
7. MIG-RESTORE: 하나의 source zip와 standalone restore.py로 Git/sibling 없는 위치에서 on/off locked build.
8. MIG-DOC: 전체 문서 색인/링크/UTF-8 LF와 파일 대응표 누락0. 자산 복사 hash/환경/모델 재검증.
9. MIG-REMOVE: P4 최종 소스·검증 결속, HF linked worktree의 변경/자산 보존과 등록 정리 뒤 원본 폴더 제거.

## Evidence

초기 반례/backup은 `.cache/hf/migration/20260914/`, 실행 산출물은 `target/hf/migration/`에 보존한다.
보고는 `tests/reports/migration/`에 명령·exit·source/binary/model hash·오류·정리 결과·미실행을 남긴다.
실패 검사를 삭제하거나 BF16 허용오차를 완화하지 않는다. 작은 Qwen 통과는 H0–H7/SLO 수용이 아니다.
