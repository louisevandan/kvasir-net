# LOAD/UNLOAD node lifecycle M4 deterministic execution plan

## Sealed scope

The baseline is `c55c9ce8f24bf17ab461e7da7dae9f4f8dbe8b8e`. M4 uses the final M3 source to LOAD real small
llama.cpp and HF models and verify normal generation, interleaved requests, cancel, UNLOAD, an empty INSPECT and a re-LOAD with a new generation.
After the run, the documents that own the lifecycle contract are migrated to the current path. Long-prompt 8-wave performance and
the Qwen3.5-122B-A10B Release A promotion are later roadmap stages and are not added to these acceptance figures.

## Pre-run verdict and execution resources

- No build or model run happens on this Windows PC. Checking model file paths, sizes and hashes and checking documents are
  done only as low-load work.
- Linux Spark `m42@192.168.0.26` uses at most 8 Cargo jobs/test threads out of 20 logical CPUs.
  Windows `M42-SERVER2` `42mob@192.168.0.29` uses at most 8 Cargo jobs out of 28 logical CPUs.
- On Spark, the existing PID `1287453` has only the process left with no `:52005` listener, so TCP INSPECT times out in both directions.
  It is not owned by this work, so it is not stopped and not judged as a normal agent or an empty node. Only a new task-owned agent is started, on a separate
  port, with its PID, start time, executable and listener sealed.
- The starting state of M42-SERVER2 is 0 `p4-agent.exe`, 0 agent listeners, and both RTX 3090 cards at
  26 MiB used and 0%. HF and Windows llama.cpp use only `CUDA_VISIBLE_DEVICES=0` or native `CUDA0`.
- The GGUF is `Qwen3.5-0.8B-Q8_0.gguf`, 833,592,736 bytes,
  SHA-256 `c54f8b67069c70085b98440de696b44da8250250ac69a961b41133def876e262`.
  The HF checkpoint revision is `2fc06364715b967f1860aea9cf38778875588b17`, and the safetensors are
  1,746,942,600 bytes, SHA-256 `04b1c301231dd422b8860db31311ab2721511346a32cb1e079c4c4e5f1fe4696`.
- The existing Linux/Windows native servers are from P4 source `bfc8bcc45c4d2f8bd8723add6c8f1190db623989`, so they are
  older than the current READY resource profile. The sealed model memory-plan was confirmed to be missing three required limit values, so
  they are not reused. On the final M4 source, rebuild and test with Windows CUDA 12.8, compute 86 and parallel 8, and
  allow LOAD only after confirming the new binary/DLL hashes and the resource profile for each stage role. A middle stage
  must have positive tensor payload/count and overall limit; the final sampling stage must have payload/count 0 and a positive overall limit.
- Ports are chosen after reading each OS's actual dynamic range and existing listeners. Only task-owned firewall rules and
  processes are removed, and a final listener count of 0 is confirmed.

## Reuse of past lessons

- Apply all of `L001`–`L040`. In particular, do not start a model LOAD until the real route preflight of L002/L003
  passes, and per L021/L028, remote multi-step commands on Linux and Windows run only transferred files.
- Deliver the latest commit to both hosts as a Git bundle. Build only after confirming the commit object per L010, a clean checkout,
  the absolute Cargo path and jobs 8. Bind the run source and binary hashes to the result.
- Check the model, Python, bundle and native DLL/SO by hash/version. Block missing dependencies in preflight instead of
  discovering them as run failures.
- Each LOAD requires `nodes=[]` before it starts. Compare the causation and typed lifecycle metadata of success, rejection and unknown results,
  and after receiving the UNLOAD terminal, confirm `nodes=[]` with a separate INSPECT.
- Do not relax the expected normal generation, cancel timing, request count or GPU selection after a failure. Do not rewrite the historical
  CREATE/DELETE records, but check that that content type count is 0 in the current run trace.

## Sealed tests

| ID | Real path | Verdict |
| --- | --- | --- |
| M4-P0 | source/binary/model/Python/GPU/port/agent/node preflight | Exact round trip to both new agents, nodes 0, task-owned child/listener 0, hash/version match |
| M4-L1 | Spark OUTER→Spark/Windows agent→real llama.cpp 2 stages | CREATE/DELETE 0, two nodes created by LOAD alone, exact `4`/EOS/release, nodes 0 on both sides after UNLOAD |
| M4-L2 | Reload llama.cpp 2 requests with a new generation of the same node ID | Different normal prompts/responses, per-request completion and release, 0 effects from the old generation, final nodes 0 |
| M4-L3/NL12 | After the first real llama stage LOAD, reject the second stage with a pinned wrong binary | OUTER UNLOADs the first node, 0 automatic reclaim of other nodes by P4, first/cleanup errors distinguished, nodes 0 on both sides |
| M4-H1 | Latest feature-on agent on M42-SERVER2 and real HF single GPU short | `4` and `서울` (English: "Seoul"), reference logits/cache match, nodes 0 and workers 0 after UNLOAD |
| M4-H2 | New-generation HF interleaved cancel | Two normal requests match the reference, the cancelled request is `cancelled_at_step_boundary`, nodes 0 after release/cleanup |
| M4-H3 | Another new-generation HF short | New worker PID/LOAD identity, same normal response and parity, old generation rejected, final nodes 0 |
| M4-R | Targeted, full Python, workspace feature off/on, docs checks | failed 0, ignored counted separately, final source identical to verified source |

M4-L1/L2 use two physical agents and per-platform natives from the same patch set. If platform-compatible readiness differs,
do not relax the configuration or expected values; preserve the original output. HF sees only one RTX 3090 CUDA0, and other GPUs and
the local desktop GPU are not used.

## Rounds and failure handling

Before round 1, pass M4-P0, a static parse of every runner, the config validator and the model expected-value postprocessor.
The first run executes L3's intended rejection and all of L1/L2/H1–H3 as one sealed bundle. On an unexpected failure, do not retry the same command.
Preserve the original logs and the source/binary/input identity, add a new lesson and automatic block to the register,
and then fix only a single cause. Round 2 is that fine adjustment; round 3 is confirmation with no functional change plus independent mutations.

The target is success within 3 rounds. If it still fails after 3, do not repeat the refuted design as is; close the cause and state transitions
again and open round 4. Round 5 is used only to confirm that redesign. After 5, do not mark it complete.

## Completion outputs

- The run output and summary go into `tests/reports/node-load-lifecycle/<timestamp>.md` and the remote raw artifact.
- Update `event-protocol-v2.md`, `layer-isolation-contract.md`, `distributed-batching-verification.md`,
  `adapter-batching-layers.md` and the HF/run READMEs for the new LOAD/UNLOAD ownership.
- Link the report from the README, document map, roadmap and lifecycle plan, and record the Qwen122B first next action and the unaccepted H0–H7.
- Make all non-ignored changes into one recoverable commit, push it, then confirm local/remote HEAD and dirty 0.
