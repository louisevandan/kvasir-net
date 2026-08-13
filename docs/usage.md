# Usage

Start an agent with its listener address. It defaults to `physical CPU cores × 2` Tokio workers; pass `--workers N` to override the count. Then start adapters with their own agent registration arguments:

```powershell
.\target\debug\p4-agent.exe 127.0.0.1:29101
.\target\debug\p4-agent.exe 127.0.0.1:29101 --workers 48
.\target\debug\p4-llamacpp.exe 127.0.0.1:19103 127.0.0.1:29101 llamacpp-stock http://127.0.0.1:19104 Qwen2.5-1.5B-Instruct-Q8_0.gguf
```

The caller never supplies `127.0.0.1:19103`. It first calls `inventory()`, creates a NodeSlot against `llamacpp-stock`, loads a deployment binding, then streams inference:

```js
const node = await controller.createNode({ adapterId: 'llamacpp-stock' });
let binding;
for await (const event of controller.loadModel({ nodeId: node.nodeId, deploymentId: 'd1', model: 'model.gguf', planRevision: '1' })) {
  if (event.type === 'model-bound') binding = event;
}
for await (const event of controller.infer({ nodeId: node.nodeId, deploymentId: 'd1', bindingId: binding.bindingId, runtimeGeneration: binding.runtimeGeneration, prompt: '러스트에 대해 설명하라.', options: { top_p: 0.9, top_k: 20 } })) console.log(event);
```

Run owned proof paths from `apps/p4`:

```powershell
.\tools\scripts\e2e\stock\run-real-e2e.ps1 -P4ListenPort 29101
.\tools\scripts\e2e\pipeline\run-pipeline-e2e.ps1 -P4ListenPort 29201 -Prompt '러스트에 대해 설명하라.' -MaxTokens 16
```

Generate and reuse the semantic-prefill fixture for the 256-session workload.
`MaxTokens` is an upper bound: a normal request can end at EOG and still pass.

```powershell
node .\tools\scripts\fixtures\generate-prefill-prompts.mjs
.\tools\scripts\e2e\pipeline\run-pipeline-e2e.ps1 -PromptFile .\fixtures\prefill-prompts-ko-400t.json -PromptOffset 0 -MaxTokens 600 -Parallel 256 -ConcurrentRequests 256 -ExecutionWindow 16 -AgentWorkers 48 -P4ListenPort 29301 -P4RingListenPort 29303
```

`ExecutionWindow` is controller-side credit, not an Agent queue: it limits
active continuous streams for large-prefill workloads while preserving the full
set of 256 unique logical sessions. Raise it only after native prefill capacity
has been measured for the selected model and context.

For overlap validation, retain an initial 48-session cohort and add requests
while those streams are still running. `ExecutionWindow` must cover the whole
arrival plan so the client does not hide a Pipeline queue behind its own limit.

```powershell
.\tools\scripts\e2e\pipeline\run-pipeline-e2e.ps1 -PromptFile .\fixtures\prefill-prompts-ko-400t.json -MaxTokens 1000 -Parallel 48 -ConcurrentRequests 48 -TotalRequests 56 -InitialRequests 48 -ArrivalIntervalMs 2000 -ExecutionWindow 56 -ExpectSharedMemory -Benchmark
```
