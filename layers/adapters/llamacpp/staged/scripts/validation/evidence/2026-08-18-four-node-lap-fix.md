# Four-node decode-lap regression

> 문서 지위 (2026-09-06): **날짜·환경 한정 증거**. 본문 날짜/커밋/모델/토폴로지의 관측이다. 현재 구현이나 다른 분산 환경의 완료 증거가 아니다.
> 현재 목표·상태·순서는 [실행 로드맵](../../../../../../../docs/distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](../../../../../../../docs/document-map.md)를 따른다.

Date: 2026-08-18

## Command

```powershell
& .\apps\p4\tools\scripts\e2e\run-ssh-forwarded-real-four-node.ps1 `
  -RunId '20260818-current-lap-fixed' `
  -AgentBinary 'F:\dev\linkcpp_product\apps\p4\target\release\p4-agent.exe' `
  -DriveBinary 'F:\dev\linkcpp_product\apps\p4\target\release\p4-drive.exe' `
  -Requests 4 -Tokens 8
```

## Topology

- central RTX 3090 + RTX 4080
- remote RTX 3090 x2 through SSH forwarding
- four P4 agents and four stage servers
- Qwen2.5-1.5B-Instruct-Q8_0.gguf

## Result

`PASS: SSH-forwarded four-node staged E2E`

The run completed 4/4 requests with 8 tokens each, no failed requests, ordered
streams, terminal responses, and cleanup. The previous run stalled at token 1
because the tail cut-set wrapper was sent back to stage 0 on the next decode
lap. `Next::Lap` now removes the wrapper and sends the original `Continue` body;
the cut-set remains restricted to the current lap's stage-to-stage handoff.

Result directory:

`target/ssh-forwarded-four-node-e2e/20260818-current-lap-fixed`
