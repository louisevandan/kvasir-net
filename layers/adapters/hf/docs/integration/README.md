# P4 HF event integration v2

Current contract. P4's `layers/adapters/hf` owns the Rust bridge, the per-model Python, the environment and the worker deployment artifacts.
The optional agent feature in the P4 root workspace is `hf-transformers`.

| Purpose | File |
| --- | --- |
| Public Rust boundary | [crate](../../adapter/README.md) |
| Test contract | [plan](../../tests/plans/p4-integration-20260914.md) |
| Earlier real-hardware evidence | [historical report](../../tests/reports/p4-integration/20260914_023000.md) |
| P4 assembly | [integration guide](../../../../../docs/hf-integration.md) |
| Migration status | [migration](../migration/README.md) |

## Build and run

The working directory is the P4 root. `--model-dir` and the Python interpreter point to the actual prepared locations.

```powershell
cargo build --locked -p p4-agent -p p4-event-drive --features hf-transformers
python layers/adapters/hf/scripts/deployment/worker/run.py target/hf/bundle-A --label A
.cache/hf/environments/qwen3_5_0_8b/Scripts/python.exe layers/adapters/hf/scripts/verification/event_qwen/run.py --host 127.0.0.1 --port 41980 --plan layers/adapters/hf/plans/qwen3_5_0_8b/balanced_two_gpu_fp32/plan.json --scenario layers/adapters/hf/scenarios/qwen3_5_0_8b/short/scenario.json --bundle target/hf/bundle-A/bundle.json --output target/hf/short
```

The agent is started as a separate process, e.g. `p4-agent 127.0.0.1:41980 tcp://127.0.0.1:41980`.
`--nodes` is a list of `{agent,node,generation}`, and `--deployments` is a list, in the same order, of remote absolute paths `{python,bundle,model_dir}`.
The bundle bytes of every worker are identical. `--agent-binary` records the hash of the consuming binary.
`epoch_eight` with `--blocks 3` reuses 8 IDs across 3 epochs within one LOAD. A rerun uses a new generation.
event_matrix increments the generation for each case and checks fixed expected normal responses.

## Transport and authority

The single `scheduling.drive` of the OUTER model controller owns step membership/issue/position and token selection.
Both the independent reference path and the event path consume this scheduler. The event controller does not open worker pipes.
Head worker results go through Rust retained completion → P4 broker → the next node's Rust → Python.
Each LOAD fixes the topology and the OUTER owner. Execution happens only after checking generation/epoch/serial and the same-job receipt chain of the preceding stage.
OUTER sends cache queries and unload to each stage, and a cache query is a read that does not consume a serial.
A receipt is a settlement record over trusted P4 transport; it does not claim cryptographic authentication or verification against malicious workers.

| Standalone v1 at the outset | P4-consumed v2 |
| --- | --- |
| The controller calls each worker pipe directly | OUTER controller → head retained → P4 broker → each stage → OUTER |
| ready/run_id | bundle/config/model/tokenizer/plan/stage/generation/dtype/operation identity |
| Cumulative active+retired cap | The same v1 state is recreated for each drained epoch, and past epochs are rejected |
| State location check | Check of all attention KV, conv and recurrent elements/composition/shape/dtype |
| Local Python execution | External Rust supervisor, P4 creation/INSPECT/lifecycle, deployment per physical host |

IPC carries v2 packets inside the existing v1 framing (magic+version+reserved+u64 BE length).
A packet is a u32 BE JSON length, a JSON object and an opaque body. JSON ≤64KiB, frame ≤32MiB, and receipts are included in the frame.
LOAD identity checks protocol/bundle SHA256/config/model/tokenizer/plan/stage/generation/dtype/boundary/operations.
stdout is for framing only, and stderr is collected separately up to ≤64KiB. The worker environment is pinned together with the Qwen runtime_check.

## Budget and lifecycle

An Agent-target `NODE_LOAD` atomically claims the node ID and creates a bounded mailbox, a supervising task and the Rust bridge.
The route is paused while loading and accepts ordinary requests only after Python worker readiness succeeds.
Input keeps the upstream original claim while counting a separate byte cap. Full/Closed returns the original allocation/claim.
Output reserves the maximum packet and the maximum route envelope before calling the worker. Held claims after dequeue still count toward count/bytes.
Scratch for copying IPC bodies is at least 4×frame. Separately, input/output storage, finite metadata with JSON ≤64KiB,
16 topologies, bounded stderr, and a file buffer of up to 16MiB during bundle verification must be accounted for. The frame limit is not an overall heap cap.
One bridge runs exactly one model command at a time. The input queue and output queue/retained limits are part of the NODE_LOAD specification.

EOF, truncated frames, wrong identity or timeout during execution are fenced as uncertain. The first input claim and the response bytes already read are
held until an explicit abort, and the same issue is not retried automatically. The façade stays alive to handle cleanup control.
Model errors and cleanup errors are kept separate. Timeouts are LOAD/command ≤120 s, graceful exit 5 s, forced reclaim 10 s.
`abort` is an explicit abandonment of the entire LOAD generation and must have epoch/serial/issue/position=0 and request="".
It differs from request cancel/release, and previous LOAD generations are rejected. Each worker can be reclaimed even if only some stages switched during a barrier.
Epoch transition and NODE_UNLOAD are rejected while earlier held output exists. A snapshot taken before the node's own final ack is reclaimed is busy.
NODE_UNLOAD success means the supervisor removed the node task, route and owner after the worker released its physical state.
A failure or unknown result is never turned into success, and the first error and cleanup error are preserved separately.

The v1 `active+retired<=8` contract is preserved. In v2, OUTER issues an explicit epoch barrier after confirming release on all stages.
Each stage creates new bounded StageSessions only at active=0. step/release/cancel from a previous epoch are rejected before execution.
A barrier rejected or left uncertain by even one stage is not treated as normal progress, and the whole LOAD is aborted.

## Python replacement and source shipping

NODE_UNLOAD A, then NODE_LOAD a new generation with the new B bundle. With the same agent hash, verify the change in bundle hash/readiness label and a normal request.
A bundle whose protocol/hash does not match is rejected before LOAD, and the run directory is not overwritten.

A single source archive is exported from a clean P4 commit. schema2 records the P4 commit, archive SHA256 and restore tool SHA256.
Existing schema1 archives of the two repositories are restored only with the restore.py of that time included in the old bundle.

```powershell
python layers/adapters/hf/scripts/deployment/source/run.py export target/hf/source-bundle
python target/hf/source-bundle/restore.py restore target/hf/source-bundle target/hf/reproduced
```

The standalone helper restores a single `reproduced/p4` and builds agent/event-drive with the feature on and off using the root lock.
No Git or sibling HF checkout is needed. A Rust toolchain and the pinned dependency packages are needed.
The Python environment and weights are not included in the source archive. The worker bundle is built separately.
The Windows venv launcher creates the real interpreter as a child. The launcher, started with CREATE_SUSPENDED, is placed in a
KILL_ON_JOB_CLOSE job before its main thread is resumed, and the job's active process count is confirmed to be 0 before the abort ack.
Other workers are not killed via a process-wide search. Inheritance of child processes by the Job object follows the
[Microsoft contract](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-assignprocesstojobobject).
Durable recovery from a sudden whole-host shutdown is not part of this in-memory contract.
