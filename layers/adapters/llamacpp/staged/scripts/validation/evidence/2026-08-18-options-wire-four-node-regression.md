# Sequence.options wire regression — 2026-08-18

> Document status (2026-09-06): **Date- and environment-scoped evidence**. These are observations for the date, commit, model and topology stated in the body. They are not evidence that the current implementation or any other distributed environment is complete.
> Current goals, status and ordering follow the [execution roadmap](../../../../../../../docs/distributed-batching-roadmap.md); document authority and reading paths follow the [document map](../../../../../../../docs/document-map.md).

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
