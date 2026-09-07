# Sequence.options wire regression — 2026-08-18

> 문서 지위 (2026-09-06): **날짜·환경 한정 증거**. 본문 날짜/커밋/모델/토폴로지의 관측이다. 현재 구현이나 다른 분산 환경의 완료 증거가 아니다.
> 현재 목표·상태·순서는 [실행 로드맵](../../../../../../../docs/distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](../../../../../../../docs/document-map.md)를 따른다.

The latest staged Release artifacts were tested after adding the optional
`Sequence.options` field to the local v2 HOP payload.

Command:

```powershell
.\apps\p4\tools\scripts\e2e\run-ssh-forwarded-real-four-node.ps1 \
  -RunId 20260818-options-wire-regression -Requests 4 -Tokens 8
```

Result: **PASS**.

- central nodes: RTX 3090 and RTX 4080
- remote nodes: two RTX 3090 stages over SSH forwarding
- requests: 4
- tokens per request: 8
- cleanup: completed; no stage process was left resident

This validates that existing options-empty HOP frames remain compatible with
the 4-node remote chain. It does not claim that request options are semantically
applied; that remains a separate per-sequence sampler slice.
