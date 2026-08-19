# Sequence.options wire regression — 2026-08-18

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
