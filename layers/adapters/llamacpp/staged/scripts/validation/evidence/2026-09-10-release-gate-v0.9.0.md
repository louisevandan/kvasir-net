# 2026-09-10 — v0.9.0 initial gate and closing re-verification

**Current status: release candidate preserved; formal seal BLOCKED.** The r256 run of the new agent/drive was interrupted by a Windows Update restart.
After login is restored, the r256 re-judgement and the out-of-memory refusal gate remain. The initial gate and the latest closing records below are kept separate.

Type: real-hardware gate for artifacts rebuilt from the release tree. This is not performance evidence.
Baseline commit `890417a77`, working tree clean. This document judges the completion condition of roadmap P-3e turn 1.
Location: `m42-server2` (RTX 3090 ×2), logged in, cards at 313 MiB before the run.

## Artifact binding

Build: prepared tree `0eadefebd3-f37f181c9d38` (26 patches, including 0026), Ninja **Release**, CUDA 13.1,
`86;89`. The first configure came out as **Debug** because the script passed only `--config` to Ninja (a pitfall recorded in memory);
after stopping, the same cache was reconfigured with `-DCMAKE_BUILD_TYPE=Release` and rebuilt. CTest **15 / 15 passed** (3.13 s).
`p4_staged_server.exe` is written to the build root while `bin/` holds only the DLLs, so the flat copy was assembled from those two plus the CUDA runtime
`cublas64_13`·`cublasLt64_13`·`cudart64_13`.

The pre-deployment remote state was preserved as `staged.pre-v090-20260909T174456Z`·`p4-agent.exe.pre-v090-20260909T174456Z`
(previous server `b66beffb…`, previous agent `eaa153f4…`).

| File | sha256 (local = remote, copied from the deployment log) |
| --- | --- |
| `p4_staged_server.exe` | `fd06c12237083c5e247108e3b3704aa9d7b205c3ee022d6e812f05c408fd9487` |
| `llama.dll` | `90f21982c2e354c92ddda16edd1de4abe1d6445e529a1ad17b85df8201480a1b` |
| `ggml-cuda.dll` | `8a8ed0d4b3938cf634e01e10ff75a475355ed61f57f0a152492aa38eab896c97` |
| `ggml-base.dll` | `1e74e53afab3806f7a2826d81ed711103110b8c43e029e5cd3101616ca420eca` |
| `ggml.dll` | `ddd78d3dbf7afb76d2da625889773769300df905ae08e3ec608916e659378671` |
| `p4-agent.exe` | `cf1c7002a3c4e3c1ee40fe4330f00bd0a51f33bac9dc24bac591fd384be5388a` |
| `p4-event-drive.exe` (local drive) | `3cdac71588c3ae0bb477c49b8617af2ac1b4ef8bd8cea2099ac41ac2df2969a4` |

Agent `cf1c7002…` differs byte-wise from the 09-09 deployment `eaa153f4…`. The only non-test source change that goes into the agent
in between is 11 lines in the `ownership.rs` test module, and the cause of the byte difference was not determined.
Each run's `evidence.json` re-records the remote hashes, so the verdicts in the table below are checked against those values.

## Gate

Artifacts are under `target/release-gate-v090/<run ID>/`, and the numbers were copied from each run's `artifact.json`, `evidence.json` and
`agent.stderr.log`. For all four runs, `evidence.json` records the remote server `fd06c122…`, `llama.dll`
`90f21982…`, `ggml-cuda.dll` `8a8ed0d4…`, agent `cf1c7002…` and launcher `2cdfc22d…` (same as 09-09),
matching the binding table above. The commit is `890417a77` for all four runs.

**Correction to the working-tree record.** #1 has `working_tree_clean=true` and #2~#4 have `false`. While the gate was running,
this document itself (untracked, 4,134 B, sha256 `6f6204d4…`) and one registration line in `docs/document-map.md` were written to the working tree,
and each run's `dirty.diff` contains only those two files. The source did not change.
What the runs sealed is the source and deployment hashes of `890417a77`; this document is the file that records the result.

| # | Scenario | Run ID | req / completed / released | error / cleanup_error | Generation TPS (reference) | Verdict |
| ---: | --- | --- | --- | --- | ---: | :-- |
| 1 | `smoke` (2B, 4 stage) | `20260909T174533Z-2a88df2b` | 1 / 1 / 1 | null / null | 17.77 | **pass** |
| 2 | `pressure` (2B, 4 stage, r256) | `20260909T174740Z-5b7f716f` | 512 / 512 / 512 | null / null | 400.19 | **pass** |
| 3 | `pressure_35b` (35B, 2 stage, r96) | `20260909T175343Z-5ca4c41f` | 512 / 512 / 512 | null / null | 278.27 | **pass** |
| 4 | `release_35b_r256` (35B, 2 stage, r256) — 0026 | `20260909T180528Z-f8972a0c` | 512 / 512 / 512 | null / null | 330.12 | **pass** — see the 0026 basis below |
| 5 | `release_35b_r512_must_refuse` | `20260909T181615Z-55018c0d` | load refused | `n_seq_max must be <= 256` | — | **invalid** — refused at the llama.cpp limit before reaching the plan, so it is not evidence of a plan refusal |
| 5′ | `release_35b_must_refuse` (r256, ctx 1024, ubatch 4096) — 0026 | `20260909T181846Z-6c01faf4` | load refused (no artifact, refused before inference) | `staged memory plan exceeds currently free memory` | — | **pass** — refused with `fits_current_free=false`. See the stage-distinction correction below |

#3 and #4: the responses were inspected directly: `<think>` leakage 0/512, ChatML marker leakage 0/512, distinct responses 375·373/512,
most-repeated 24-character window 0.06·0.11 (markdown structure), and every stop was `length` (fixed-length load test).
Peak VRAM (maximum `memory.used` within the window) was **15,619 MiB** for #3 and **21,459 MiB** for #4 (87 % of 24,576).

### 0026 verdict basis (plan records in each run's `agent.stderr.log`, `plan-lines.mjs`)

| Run | n_seq | planning pass `CUDA0 RS buffer` | `free` | `context` | `required` | `fits_current_free` |
| --- | ---: | ---: | ---: | ---: | ---: | :-- |
| 09-09 `…e6c5c4c4` (0025 tree) | 256 | 7,504 MiB | 15.43 | 7.99 | 19.72 | false |
| #3 (0026 tree) | 96 | **0.00 MiB** | 22.76 | 3.00 | 14.01 | true |
| #4 (0026 tree) | 256 | **0.00 MiB** | **22.76** | **7.99** | **19.72** | **true** |
| 5′ head `MEMORY_PLAN` | 256 · ctx 1024 · ubatch 4096 | **0.00 MiB** | 22.76 | 8.66 | 22.64 | **true** (headroom 0.12) |
| 5′ head `MEMORY_ACTUAL` | same | 7,504 MiB (actual) | 0.11 | 8.66 | **22.64** | allocation size matches the plan |
| 5′ tail `MEMORY_PLAN` | same | **0.00 MiB** | 22.76 | 8.66 | **23.64** | **false → refused before the actual load** |

#3 and #4 both log, in the planning pass (plan #1), `CPU RS buffer` and `CUDA0 RS buffer` as **0.00 MiB**, and in the following
actual pass (`MEMORY_ACTUAL`, record #2) the same 2,814 MiB and 7,504 MiB as on 09-09 are actually allocated. `context` is reported as
3.00 and 7.99 GiB, the same as with 0025, so **the expected-size report including alignment was kept**, and `free` is 22.76 GiB, the card's initial
free memory. The #4 configuration, refused on 09-09 with `required 19.72 > free 15.43`, was **admitted** with the same `required 19.72`
and `free 22.76`, and then completed the actual load, 512 inferences, release and UNLOAD.

Pass-condition verdict. #4: planning pass RS **0.00 MiB** ✓, `context` expected size kept ✓, `fits_current_free=true` ✓,
actual load, inference and UNLOAD ✓, peak VRAM 21,459 MiB recorded ✓.
5′: **refused** — the server emitted `staged memory plan exceeds currently free memory`, the stage ended with exit 5, and
the drive failed at the load step (no `artifact.json` because this was before inference, as the preservation contract says). The fit check
`required() <= free` is still in place and actually blocks configurations that overflow. r512 was refused at
`n_seq_max must be <= 256` before reaching the plan, so it was invalid for this purpose.

**Closing audit correction — the under-reporting claim is withdrawn.** The earlier record misread the head's `MEMORY_PLAN` and the tail's
`MEMORY_PLAN` as a plan/actual comparison for the same stage. The actual order in the log is head PLAN → head ACTUAL →
tail PLAN. The head's model/context/compute are 11,276,284,416 / 9,294,577,664 /
3,742,433,280 B, identical between plan and actual, and the tail's separate plan is 11,923,591,680 /
9,294,577,664 / 4,169,138,176 B. The tail was refused with `required 25,387,307,520 > free 24,436,015,104 B`
before the actual load. This run has no `MEMORY_ACTUAL` for the tail.

`load.rs` loads the stages of the same agent in order. `llama_stage_runtime.cpp` prints the plan as
`MEMORY_PLAN` and the actual allocation as `MEMORY_ACTUAL`; in the actual comparison it does not re-run the fit check on free,
but checks whether model/context/compute match. A drop in free after allocation is not evidence of under-reporting.
So the "deviation" of model 0.60 GiB and compute 0.39 GiB is a cost difference between different stages, and it is removed
from the known defects. #3 and #4 must also be checked by pairing each stage's PLAN/ACTUAL.

The analysis tool was fixed: it skipped ACTUAL and attached the preceding actual RS allocation log to the next PLAN.
`plan-lines.test.mjs` checks, through the real CLI, head PLAN/ACTUAL/tail PLAN, ACTUAL alone, and a corrupted ACTUAL.
3 failures before the fix → 3 passes after the fix; removing ACTUAL handling in a separate copy gives 3 failures.
The SHA256 of the Node executable and of each interpreted source, and the raw failure/pass logs, are preserved in the closing raw data.

## Closing re-verification — source 3302591fc, formal seal BLOCKED

Feature and test fix commit `3302591fc7e2a69298cfa0b2e9c9378629cf293b`; all four final attempts were on a clean tree.
The later candidate-preservation commit changes only documents, the manifest and the line-ending rule for hash files. The existing unpublished v0.9.0 tag is kept as an archive ref,
and it is not re-sealed as the formal tag while the required gates for the new binaries remain. Nothing was pushed.

### Completed fixes and verification

- `plan-lines.mjs`: records the PLAN/ACTUAL kind as is and clears the buffer log at each boundary.
  Fixed the error that attached the actual head RS to the next tail PLAN. 3 real-CLI regressions: 3 failures before the fix → 3 passes after the fix;
  removing ACTUAL handling in an independent copy gives 3 failures.
- `build-stage-server.mjs`: passes `CMAKE_BUILD_TYPE` to configure and copies cublas/cublasLt/cudart next to the actual server exe.
  Added 4 regressions that run the real builder and replace only the external process/filesystem boundaries.
  Independent mutations: removing the configure hookup gives 2 failures; pinning the copy location to the Release folder gives 1 failure.
  The SHA256 of all interpreted sources and the Node exe, and the failure/success logs, were preserved.
- Re-ran the official builder on the real CUDA Release cache: Release configure, Ninja `no work to do`, **CTest 15/15**,
  runtime copy complete. The native source and the 26 compat patches did not change after the initial CUDA build.
- The version 0.9.0 Rust agent/drive was rebuilt in a separate `CARGO_TARGET_DIR` with `cargo build --release --locked
  -p p4-agent -p p4-event-drive`. The new agent was deployed to the remote host and the real-hardware runs below used the new drive.
- Final full-workspace run: **1,374 passed / 0 failed / 7 ignored / 0 filtered**.
  The first run had 1 docs_lint failure because of mixed line endings in a README; after the CRLF fix the full rerun passed.
  Both `workspace.log` and `workspace-final.log` were preserved.
- Node total **146 passed / 0 failed / 0 skipped**. The scope matching the earlier report went 94→101 (+7 regressions);
  the remaining 45 are compat/upstream validator tests added to this tally.
  docs-lint **91 files clean**, compat manifest and patch classification **26 valid**, private headers **81 clean**.

| File | Actual deployed/run SHA256 |
| --- | --- |
| `ggml-cuda.dll` | `8a8ed0d4b3938cf634e01e10ff75a475355ed61f57f0a152492aa38eab896c97` |
| `llama.dll` | `90f21982c2e354c92ddda16edd1de4abe1d6445e529a1ad17b85df8201480a1b` |
| `p4_staged_server.exe` | `fd06c12237083c5e247108e3b3704aa9d7b205c3ee022d6e812f05c408fd9487` |
| `p4-agent.exe` | `4316965abe718bf2feb6b895586824f3b51a74512ae69110699a2e4724d9a543` |
| `p4-event-drive.exe` | `44cc922603ddd1b5e4874590cd2bcb6b595d70a9b90c6e456197ec2c8cda8bf3` |

The authoritative fields are the remote image `evidence.remote.images` and the local drive `evidence.binaries.drive_sha256`.
The generic `binaries.server_sha256` and `ggml_cuda_sha256` hold the old local cache values b66beffb/3452e117, so do not read them as the remote run images.
The 10 staged files/DLLs and the agent/drive hashes in `deployment.json`, each run's actual remote images, source and clean state, and
the original MANIFEST were re-checked with `audit-runs.mjs`.

### Latest run results

| Scenario | Run ID | req/completed/released | Verdict |
| --- | --- | --- | --- |
| smoke | `20260909T190252Z-b9e2cef9` | 1/1/1 | PASS, error/cleanup_error null |
| pressure (2B r256) | `20260909T190458Z-bfde8e1d` | 512/512/512 | PASS, error/cleanup_error null |
| pressure_35b (r96) | `20260909T191117Z-853157c9` | 512/512/512 | PASS, error/cleanup_error null |
| release_35b_r256 | `20260909T192319Z-f82b070a` | 512/0/0 | failure interrupted by a planned OS restart; needs re-judgement |
| release_35b_must_refuse | not run with the new binaries | — | BLOCKED, login recovery required |

The latest r96 gives 102,400 generated tokens / 385.142 s = 265.88 raw TPS, and the maximum `memory.used` over the full GPU capture is
15,646 MiB. The denominator is `artifact.elapsed_ms` and the numerator is the OUTPUT token count; this is not a quality-passing TPS or an improvement rate.
smoke has 1 stop at the 400-token length, and each of the two pressure runs has 512 stops at the 200-token length.
For every stage that finished allocation in the initial passes, the latest passes and the latest interrupted run, the PLAN/ACTUAL of the same stage were paired and host/device
model/context/compute were compared byte for byte; they match. This is a point comparison for these configurations, not non-regression across all models/backends.

### Cause of the r256 interruption and partial results

The run preserved as a partial artifact 512 delivered, 0 completed/released, and **16,896 approved OUTPUT tokens**.
The first inference error is os error 10054 on a stage connection, and a cleanup_error was also left separately.
Because of `evidence_missing={requests:512,stage_executions:2}`, the per-request row count is 0; do not confuse that with the output token evidence.
Both stages finished loading and their PLAN/ACTUAL values matched. Peak VRAM over the full capture of the interrupted run is 21,486 MiB.
The partial TPS of this run is not used for throughput approval.

Windows System event 1074, as logged:

- **19:29:29.082 UTC**: `MoUsoCoreWorker.exe`, planned OS service pack restart (0x80020010), SYSTEM.
- 19:29:29.582 UTC: nvlddmkm event 153. Do not jump to the conclusion that this is the cause of a GPU fault.
- **19:32:21.868 UTC**: `TrustedInstaller.exe`, planned OS upgrade restart (0x80020003).
- Boot and SSH recovery after 19:32:56 UTC. Confirmed 0 logged-in users and 0 agent/stage processes.

This attempt is therefore classified as **a failure interrupted by a planned Windows Update restart**. It is not counted as a pass and not deleted.
The XML and timestamps are preserved in `r256-restart-initiator.json`, `r256-driver-events.json` and `r256-host-restart.json`.
Entries where WER re-reported an old dump during the first investigation are not used as evidence for this BlueScreen.
The ENOENT raised when a one-off closing wrapper read the missing `evidence.json` is a follow-on error; the first error is in the preserved artifact.

### Candidate archive and resumption

- Raw data for **10 runs** (6 initial, 4 closing) is preserved, including the invalid r512 and the OS-restart interruption.
  The 10 MANIFEST copies in Git `bundles/v0.9.0/` hold 75 file hashes, and
  `sha256(SHA256SUMS)=e57b49a793512aba19328b0c6876687221def3fe8096cef09c58b6d1d932d959`.
- The candidate runtime/source/evidence ZIPs and `RELEASE-STATUS.json`·`SHA256SUMS` are stored in
  `F:/dev/p4-releases/v0.9.0/` and on the remote host in `C:/Users/42mob/p4-remote/releases/v0.9.0/`.
  The Microsoft VC++ x64 runtime/Windows UCRT and NVIDIA driver that the runtime actually imports are environment dependencies;
  the CUDA DLLs, dependency licences and internal verification scripts are included. Model weights are not included.
- The existing tag object is preserved in `refs/archive/v0.9.0-pre-close-20260910` and in the raw data.
  The source commit of the candidate archive is bound by the external status JSON, and **the formal release tag/push is on hold**.
- **First action on resumption:** confirm 42mob interactive Windows login and S: access → check OS/driver/file hashes →
  re-judge r256 and run must_refuse with the same binaries/scenarios → record the new results together with the current failure → final annotated v0.9.0.
  Windows Update policy, drivers and login settings were not changed.

H0~H7, multiple physical hosts, normally completed responses, service approval and TPS improvement are not yet achieved.
Next-version development moves on in this order: remaining no-alloc model/backend matrix → B2/B3 → normal responses/cause trace → H5.
