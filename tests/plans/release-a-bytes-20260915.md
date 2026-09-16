# Test Plan: Release A A-BYTES

## Created

2026-09-15 KST. The baseline model is the Qwen3.5-122B-A10B UD-Q5_K_S 3-shard build, and the baseline topology is
Spark `[0,24)` / Mac20 `[24,36)` / Mac21 `[36,48)`, resident 8, total context 819,200,
batch/ubatch 128/64.

## Goal

Compute the physical results produced by native execution, the completions preserved by the adapter, and inter-agent hop receipts and edge frames
as separately owned costs. Secure the needed retention space before the work that causes it, and reject missing, infinite, negative, overflowing or
larger-than-remaining configurations before execution. Do not mistake a count for bytes, and do not use the 2 GiB wire frame default as a memory
budget.

## Environment

- Development baseline: Windows `F:\dev\p4`, `main`, source after the A-LOAD baseline `9b2522dae`.
- Native rebuilds and model verification run on remote hosts.
- The build path on this PC is blocked by default. Only when the user separately authorizes an emergency run,
  use `P4_ALLOW_LOCAL_BUILD=1` and the resource wrapper, and on the 48 logical CPUs cap
  `CARGO_BUILD_JOBS`, Cargo `--jobs`, `CMAKE_BUILD_PARALLEL_LEVEL` and `RUST_TEST_THREADS` at 8 jobs.
  Do not interpret an affinity/job ratio as a power consumption ratio.
- If inference on this PC is unavoidable, expose only the RTX 3090 with `CUDA_DEVICE_ORDER=PCI_BUS_ID` and
  `CUDA_VISIBLE_DEVICES=GPU-38e6dbac-fee5-ac16-62d4-cfacbe02f8ed`.
- The unexpected shutdowns on 2026-09-15 were at 15:19:04 and 16:18:52. 3 min 47 s before the first shutdown, two unrestricted
  `cargo test` runs had been started at the same time, and no model was running. Before the second shutdown, an unrestricted release build was
  followed by consecutive local llama.cpp and HF inference. Both Kernel-Power 41 events have bugcheck and power button time
  0, with no preceding WHEA/GPU errors. The first case independently points at build load and the second mixed build and inference,
  so at the time builds were limited to 33 logical CPU affinity and local inference was set to expose only the designated RTX 3090.
  However, the fourth consecutive independent Rust cold build, started at 23:39:18, also had its log
  cut off at 23:39:34 and rebooted at 23:40:48. No model/CUDA run happened in this window. The new Kernel-Power 41 also has bugcheck,
  power button and WHEA boot error all 0. The policy that treated a thread ratio as a power ratio is therefore discarded, and
  builds on this PC are blocked by default.

## Preconditions

1. `git status --short` is empty and HEAD equals `origin/main`.
2. The new agent namespace has INSPECT `nodes=[]`, and the existing `:52005` agent and the installed app are not changed.
3. The source/patch/model/cut/shape/allocation hashes of A-PLAN are unchanged.
4. The product input limits are 64 concurrent requests, 128 MiB request retention, 6,553,600 input tokens, 2 MiB per request,
   and 2,048 output tokens per request. The cumulative output reservation is 131,072 tokens.

## Steps

1. **B0 Pin the cost unit**
   - Across every allowed maximum UBATCH graph built by the native no-alloc graph reserve, sum the outgoing cut tensors'
     descriptor count, alias-deduplicated payload bytes and the physical-v4 fixed/row/owner metadata limit
     using checked integers.
   - PLAN and READY report the same `max_physical_result_bytes`. If the actual result encoder or the Rust decoder
     exceeds this value, reject it before body allocation/native commit.
2. **B1 Bind the LOAD profile**
   - Put the request count/bytes/input/output token limits, the physical result limit and the completion payload limit
     into the versioned resource profile owned by the llama adapter.
   - LOAD fails closed on a missing profile, 0, overflow, PLAN/READY mismatch, or insufficient mailbox/edge/receipt headroom.
   - Do not put llama.cpp tensor types or model shapes into the P4 core.
3. **B2 Reserve retention before execution**
   - Before the actual first/middle physical call, reserve in the completion mailbox, as one group, the count/retained-byte limit
     for 1 forward plus the entire observation fan-out.
   - Compute the actual Event cost from envelope capacity and payload capacity without allocating. Publication moves the same
     reservation and checks that the actual cost is within the limit.
   - On reservation failure, scheduler/flight/KV/native/ID/effect/output must all be unchanged.
4. **B3 Separate lifetimes**
   - INSPECT the snapshots of pending requests, retained completions, broker dedupe receipts, hop receipts/outstanding and native response
     buffers as separate fields.
   - The destination commits only after reserving the actual Event cost, and the source keeps the original and
     its own claim until the remote receipt. duplicate/late/uncertain never produce an effect twice.
5. **B4 Boundaries and mutations**
   - Run small count/large bytes, the exact boundary and ±1, queue full with receipts held, duplicate/late receipts,
     uncertain transport and a neutral fake adapter on the real consuming path.
   - Recompile each of the mutations (remove reserve-before-native, commit-before-reserve, remove response bound, remove the independent receipt cost)
     in its own independent copy and confirm that the original counterexample fails.
6. **B5 Regression and real-hardware run**
   - After the targeted Rust/C++/Python tests, run `cargo test --workspace --no-fail-fast` to completion with the feature off and
     on. Count ignored tests separately.
   - Re-confirm normal generation, cancel, reclaim and re-acceptance for the small llama.cpp and HF/Python adapters.
   - With the current source/binaries, run a Qwen122B 3-host LOAD, then the boundary rejections and one normal request; UNLOAD/DELETE every stage,
     then confirm that nodes/listeners/native/GPU occupancy in the new namespace are 0.

## Expected Results

- Every limit can be recomputed from checked integers in PLAN/codec/profile, with no empirical multipliers and no default 2 GiB.
- Boundary overruns are rejected before native/KV/output, and the request ledger, reservations, credit, receipts and output effects are
  identical before and after.
- A normal result moves from the source retained claim to the destination claim and is retired exactly once after the remote receipt.
  uncertain is never promoted to success.
- The llama.cpp and HF/Python adapter regressions all pass.
- A-BYTES is promoted to GREEN only after every stage up to a normal Qwen122B response passes.

## Logs To Capture

- HEAD/source archive/native binary/model shard/plan/profile SHA-256.
- The result bound in native PLAN/ACTUAL/READY and the executed result bytes.
- request/completion/broker receipt/hop receipt/outstanding snapshots before and after rejection.
- The first error, `evidence_missing`, `cleanup_error`, exit codes and the full workspace summary.
- Per remote host: agent/native PIDs, full commands, GPU UUID, LOAD/UNLOAD/DELETE and the final INSPECT.
- Per mutation: source/binary hashes, evidence of actual recompilation, and the expected failure point.
