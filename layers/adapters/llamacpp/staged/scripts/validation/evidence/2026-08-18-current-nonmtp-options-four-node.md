# Current non-MTP options four-node regression

The CUDA stage server was rebuilt after the request sampler extension
(`grammar_lazy`, `grammar_triggers`, `preserved_tokens`, and
`generation_prompt`) and copied to the test nodes without compiling there.

- central: RTX 3090 + RTX 4080
- remote: RTX 3090 x2 over SSH forwarding
- model: `S:\models\Qwen2.5-1.5B-Instruct-Q8_0.gguf`
- workload: 4 concurrent requests, 8 tokens each
- result: `PASS: SSH-forwarded four-node staged E2E`
- evidence: `target\ssh-forwarded-four-node-e2e\20260818-current-nonmtp-options`

This is the current ordinary staged Load/HOP/Decode/Unload regression. MTP and
speculative decoding were not requested or executed. It does not claim the
23/11 GiB boundary, logits equality, or long-run stability.

After splitting the request grammar parser to keep the authored C++ source
under 400 lines, the CUDA artifact was rebuilt and the same harness was run
again with run id `20260818-current-split-options`; it also returned `PASS`.

The current CPU artifact was also exercised through the Rust concrete adapter
with Qwen2.5 1.5B. The real-server test passed HELLO, prefill, decode
(`token=21927`, `position=1`), KV save/restore/drop, and UNLOAD. The test was
corrected to model the real decode-lap rule: stage zero does not re-inject the
previous tail cut-set into the same full-range context.

After the KV manifest v2 change, the CUDA stage server was rebuilt and copied
to the same topology without a remote build. The 3090+4080 central host and
remote RTX 3090 x2 completed 4 concurrent requests with 8 tokens each:

```text
PASS: SSH-forwarded four-node staged E2E
evidence: target/ssh-forwarded-four-node-e2e/20260818-kv-manifest-p1
```

This revalidates the ordinary non-MTP Load/HOP/Decode/Unload path after the
manifest change. It does not claim MTP or speculative execution.
