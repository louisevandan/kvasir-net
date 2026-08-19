# Final four-node remote regression

Date: 2026-08-18

The latest rebuilt Release artifacts were used:

- central RTX 3090 + RTX 4080
- remote RTX 3090 x2 over SSH forwarding
- Qwen2.5-1.5B-Instruct-Q8_0.gguf
- 4 requests, 8 tokens per request

Result:

`PASS: SSH-forwarded four-node staged E2E`

The driver completed discovery, node creation, distributed load, staged HOP,
ordered terminal responses, and unload/cleanup for all four requests.

Result directory:

`target/ssh-forwarded-four-node-e2e/20260818-final-four-node`
