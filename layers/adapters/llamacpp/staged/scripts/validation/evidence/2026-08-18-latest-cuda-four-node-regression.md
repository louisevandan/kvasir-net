# Latest CUDA four-node regression

> Document status (2026-09-06): **Date- and environment-scoped evidence**. These are observations for the date, commit, model and topology stated in the body. They are not evidence that the current implementation or any other distributed environment is complete.
> Current goals, status and ordering follow the [execution roadmap](../../../../../../../docs/distributed-batching-roadmap.md); document authority and reading paths follow the [document map](../../../../../../../docs/document-map.md).

The latest CUDA stage server was rebuilt from the current staged C++ sources
and copied by the existing four-node SSH-forwarding harness. No remote build
was performed. The topology was:

- central RTX 3090 and RTX 4080;
- remote RTX 3090 x2 over SSH forwarding;
- Qwen2.5 1.5B, four layer ranges `[0,7)`, `[7,14)`, `[14,21)`, `[21,28)`;
- 4 concurrent requests, 8 generated tokens each.

Command:

```powershell
.\apps\p4\tools\scripts\e2e\run-ssh-forwarded-real-four-node.ps1 `
  -RunId 20260818-latest-cuda-options `
  -ArtifactDirectory F:\dev\linkcpp_product\.cache\staged-server-cuda-real-20260818\Release `
  -AgentBinary F:\dev\linkcpp_product\apps\p4\target\release\p4-agent.exe `
  -DriveBinary F:\dev\linkcpp_product\apps\p4\target\release\p4-drive.exe `
  -Requests 4 -Tokens 8
```

Result:

```text
PASS: SSH-forwarded four-node staged E2E
evidence=target/ssh-forwarded-four-node-e2e/20260818-latest-cuda-options
```

This proves the latest ordinary staged Load/HOP/Decode/Unload path across the
3090x2 remote node and all four nodes. It does not claim distributed MTP or
speculative execution; those capabilities remain explicitly disabled.
