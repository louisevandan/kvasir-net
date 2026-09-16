# Four-node decode-lap regression

> Document status (2026-09-06): **Date- and environment-scoped evidence**. These are observations for the date, commit, model and topology stated in the body. They are not evidence that the current implementation or any other distributed environment is complete.
> Current goals, status and ordering follow the [execution roadmap](../../../../../../../docs/distributed-batching-roadmap.md); document authority and reading paths follow the [document map](../../../../../../../docs/document-map.md).

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
