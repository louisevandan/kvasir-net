# 2026-09-07 — HEAD `2ed9b71d4` verification and the 3090×2 real-hardware ladder

Type: HEAD compile, test and mutation verdicts, plus development-harness measurements. This is not final multi-computer proof.
Target source: `2ed9b71d42e5af55a08a75fb2d7f685e038c0874` (started from a clean working tree).
The current work order is owned by the [roadmap](../../../../../../../docs/distributed-batching-roadmap.md),
and the verdict criteria by the [verification convention](../../../../../../../docs/distributed-batching-verification.md).
Every number below was copied by script from `target/p4-4node/runs/<run>/report.json`, `gpu.csv` and `evidence.json`
(`runtable.mjs`, in this session's scratchpad). The raw data under `target/` is preserved on this machine.

## 1. HEAD compile and test status

| Item | Result |
| --- | --- |
| `cargo build --workspace --release --locked` | success (warnings only) |
| `cargo check --workspace --all-targets --locked` | **failed** — `p4-llamacpp-staged-adapter` lib test, `v2/node/issue_witness_tests.rs:409` `E0594`: `RequestState` implements only `Deref` (immutable input sharing from WIP `6fe10eb10`), but the test assigns directly with `.template.envelope.return_route = None` |
| `cargo test --workspace --no-fail-fast --locked` | because of the compile error above, **0 tests run, exit 101** |
| `cargo test --workspace --exclude p4-llamacpp-staged-adapter --no-fail-fast --locked` | 849 passed / 0 failed / 0 ignored, 50 summary, exit 0 |
| Worktree (separate checkout, HEAD + a 1-line test fix `.input_mut_for_test()`) `cargo test -p p4-llamacpp-staged-adapter --locked` | 512 passed / 0 failed / 7 ignored, exit 0. `event_actor_ring_saturated_normal_ingress_must_progress_without_external_dequeue` **ok**, cap8 control ok |
| Harness `node --test test/benchmarks/p4-4node/*.test.mjs` | 72 passed / 0 failed |
| `node tools/scripts/docs-lint.mjs --all` | 79 files clean |
| CUDA staged server CTest (HEAD source, Release) | 15/15 passed |

The test fix was made only in an independent worktree, not in the repository working tree. HEAD itself still does not
compile its lib tests. The total of 849+512=1361 passed is the sum of two runs, not a single `--workspace` tally.

### 1.1 Mutation verdict for the cap1 deadlock candidate

In a worktree where `EventNode::forward_independent_front` of `2ed9b71d4` was disabled to return `Ok(())` immediately,
the same two tests were run.

- cap1 `..._must_progress_without_external_dequeue`: **FAILED** — `actor_ring.rs:288` "bounded actor-ring state
  observation" (more than 512 observations), then `:377` "actual EventNode stopped". It was cut off at the limit
  before reaching the final `normal_progress` assertion.
- cap8 `..._completes_with_capacity_eight`: ok.

A mutation that reverted all 6 candidate runtime files to `2ed9b71d4~1` ended in a compile error, because the test wrapper implements the new trait methods
(`peek_completion`, `try_take_completion_matching`); it is not counted as a detection.
So only 1 semantic mutation kind confirmed "removing the candidate makes cap1 fail". The test file changed by +118/-49 since the RED seal
`393a6c23e`, and the final `assert!(normal_progress, ...)` is unchanged on both sides.

## 2. Real-hardware binary identity

| Item | Value |
| --- | --- |
| `p4_staged_server.exe` (HEAD source, Ninja, `CMAKE_BUILD_TYPE=Release`, sm 86;89) | `337b09abe257762b053e6d6e70517e2678704a819830437e7ebcfc9d151b4c34` |
| `ggml-cuda.dll` | `ee8de6e0ee017c433f564df246743cf5650808980cf767c14feff81f2eb56e15` |
| `p4-agent.exe` (release) | `bca4da35c9affdbe1330f79414b078e5cc8f7271657f1bb7a2114ecbc7159b29` |
| Previously deployed exe (09-04, identical locally and remotely) | `821694ef4d13d0dff409d825303b91d6337cbcb69e935a52de8eb7206e1a84c9` — built before the server C++ change in `2e9451a5c` |
| Remote launcher `run-agent-42003.cmd` | `675ef882aae5dc609cab2f6da2f268187c01a6c291a82f2da496529f300f7329` (same as the 09-04 baseline: `PREFILL_FRAGMENTS=4`, min/max 0) |

When the build script uses the Ninja generator, only `--config Release` is passed and a **Debug** build is produced.
The Debug smoke gave 4.37 gen TPS (`20260907T084050Z-90e71e65`) and was excluded from the performance evidence.
Remote deployment was checked after scp with `Get-FileHash` against the hashes above, and each run's `evidence.json` carries the same values.

## 3. Measurement results

Local `hikaTR` is RTX 3090 + RTX 4080 (asymmetric, local agent default knobs); remote `M42-SERVER2` is
RTX 3090×2 (one physical host, launcher knobs as above). The TPS of the two targets is not compared with each other.
For remote utilisation, the sampler reported the same `peak` value for both GPUs.

`gen TPS` in the table is `generation_tps` from `run.mjs::metrics`: generated output **before the quality verdict**
divided by the time until the drive finished release. It is not H4's `useful_generation_tps`.
`ubatch fill%` is a phase-mixed average combining prefill and decode.

`NOT_ACCEPTED(agent_stopped)` is not evidence that the agent failed to stop. `run.mjs::stopChild`
requires `exited && child.exitCode !== null`, but a child killed by a signal has `exitCode=null` and
`signalCode='SIGTERM'`. Applying the same function shape to an independent child received the exit event
in 13 ms and still returned false. The same value in the 09-03 local runs may be the same misjudgement. The remote arms show true because
remote stop is done by an SSH script and does not take this path.

| run | scenario | target | structure | meaning | acceptance | gen TPS | rows/s(legacy total) | physical batch | rows/batch | ms/batch | mixed | ubatch fill% | GPU util mean/p50/p90/zero/peak |
| --- | --- | --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| `20260907T085616Z-9fea9f43` | smoke | local 3090+4080 | 1/1 | 1/1 | NOT_ACCEPTED(agent_stopped) | 18.61 | 19.45 | 400 | 1.04 | 53.7 | 0 | 0.2 | GPU0 7.9%/p50 0/p90 34/zero 73%/peak 1133 MiB; GPU1 10.3%/p50 4/p90 33/zero 5%/peak 5433 MiB |
| `20260907T085749Z-5d89fa09` | prefill_mix_2stage | local 3090+4080 | 192/192 | 192/192 | NOT_ACCEPTED(agent_stopped) | 328.85 | 813.08 | 915 | 103.76 | 127.6 | 113 | 20.27 | GPU0 15.6%/p50 0/p90 53/zero 55%/peak 7584 MiB; GPU1 19.3%/p50 5/p90 50/zero 3%/peak 10100 MiB |
| `20260907T090201Z-e02437f1` | pressure | local 3090+4080 | FAILED | - | drive exited 1 — unload is busy | - | - | - | - | - | - | - | GPU0 15.2%/p50 9/p90 40/zero 46%/peak 8131 MiB; GPU1 19.1%/p50 9/p90 46/zero 6%/peak 10755 MiB |
| `20260907T090450Z-50ce3487` | prefill_mix_35b_2stage | local 3090+4080 | 64/64 | 60/64 | NOT_ACCEPTED(meaning,agent_stopped) | 122.43 | 182.21 | 2467 | 22.58 | 123.9 | 0 | 4.41 | GPU0 15.0%/p50 2/p90 63/zero 45%/peak 12722 MiB; GPU1 16.5%/p50 6/p90 53/zero 1%/peak 15517 MiB |
| `20260907T092446Z-5f9b5e0c` | smoke | remote 3090x2 | 1/1 | 1/1 | accepted | 17.26 | 18.04 | 400 | 1.04 | 57.9 | 0 | 0.2 | GPU0 7.3%/p50 0/p90 32/zero 62%/peak 3655 MiB; GPU1 8.0%/p50 1/p90 33/zero 46%/peak 3655 MiB |
| `20260907T092648Z-0b5e6989` | prefill_mix_2stage | remote 3090x2 | 192/192 | 192/192 | accepted | 189.75 | 469.15 | 3020 | 31.44 | 67 | 71 | 6.14 | GPU0 24.9%/p50 28/p90 44/zero 22%/peak 8324 MiB; GPU1 28.9%/p50 31/p90 46/zero 18%/peak 8324 MiB |
| `20260907T093145Z-3bf475d1` | pressure | remote 3090x2 | FAILED | - | drive exited 1 — unload is busy | - | - | - | - | - | - | - | GPU0 14.7%/p50 4/p90 42/zero 44%/peak 9107 MiB; GPU1 21.1%/p50 13/p90 55/zero 28%/peak 9107 MiB |
| `20260907T093529Z-8d1cb2a8` | prefill_mix_35b_2stage | remote 3090x2 | FAILED | - | drive exited 1 — node already exists | - | - | - | - | - | - | - |  |
| `20260907T093641Z-5eeea50c` | prefill_mix_35b_2stage | remote 3090x2 | 64/64 | 60/64 | NOT_ACCEPTED(meaning) | 117.07 | 173.16 | 2459 | 22.95 | 132.5 | 0 | 4.48 | GPU0 16.1%/p50 3/p90 63/zero 35%/peak 13936 MiB; GPU1 16.7%/p50 3/p90 57/zero 29%/peak 13936 MiB |
| `20260907T094744Z-91d0cd3d` | vram_31b_2stage | remote 3090x2 | 32/32 | 32/32 | accepted | 71.59 | 124.3 | 1630 | 13.63 | 109.7 | 22 | 2.66 | GPU0 18.7%/p50 3/p90 48/zero 23%/peak 12858 MiB; GPU1 15.0%/p50 3/p90 42/zero 33%/peak 12858 MiB |
| `20260907T095536Z-fe64b567` | offload_35b_moe_2stage | remote 3090x2 | FAILED | - | drive exited 1 — deadline expired with observation evidence Missing { requests: 32, stage_executi | - | - | - | - | - | - | - | GPU0 11.9%/p50 2/p90 45/zero 30%/peak 3798 MiB; GPU1 11.7%/p50 2/p90 45/zero 37%/peak 3798 MiB |

### 3.1 Observations

- The remote 35B VRAM-only result of 117.07 gen TPS before the quality verdict matches the 09-04 baseline (116.9~118.5).
  There is no evidence that this HEAD change raised or lowered throughput. However, this value is a non-regression observation for comparison,
  not a paired A/B, and not an H5 promotion verdict. The same run computed with the old 09-04 formula (decode rows/s)
  gives 116.87. Recomputed without the 4 rejected requests it is **109.70**, and that is not an H1/H4
  approval metric either. The 4 rejections behind meaning 60/64 are 1 repetition degeneration (req-015, repeat 0.96) and
  3 with a Hangul ratio of 0.04~0.11. This arm sends gemma-4 turns to a ChatML model (GGUF `tokenizer.chat_template` is ChatML,
  EOS `<|im_end|>`).
- **GPU utilisation can only be read once the analysis window is stated.** The full `gpu.csv` includes the model load and cleanup
  periods. The same CSV, cut from the minimum `ingress_unix_ms` to the maximum `forward_unix_ms` of each run's stage spans,
  is listed alongside (samples are based on the sampler timestamp interpreted as KST).

  | Remote arm | Full capture GPU0/1 | Run window GPU0/1 | Run-window 0% samples | Run-window sample count |
  | --- | ---: | ---: | ---: | ---: |
  | 2B `prefill_mix_2stage` | 24.9 / 28.9% | 32.9 / 38.1% | 4.0 / 3.2% | 775 |
  | 35B VRAM-only | 16.1 / 16.7% | 29.3 / 30.7% | 24.2 / 13.6% | 1,251 |
  | 31B VRAM-only | 18.7 / 15.0% | 42.5 / 34.4% | 1.0 / 2.9% | 688 |
  | 35B expert→CPU | 16.8 / 16.7% | 21.8 / 21.6% | 28.6 / 29.1% | 3,345 |

  This is an analysis-window correction, not a performance change. **There is no joined trace showing the device idle
  while ready, legal rows exist.** The ubatch fill is also a phase-mixed average, so it is not by itself evidence of idling. Splitting the physical batches of 35B
  VRAM-only gives a decode average of 15.80 rows (2,410 batches) and a prefill average of 374.37 rows (49 batches);
  with resident 32, even if all ordinary decode is ready, that is 6.25% of 512. The idle causes were not decomposed (H5).
- **pressure (256 parallel, 512 requests, 4-stage)**: On 09-03 it passed 5 times (398~436 gen TPS), but at HEAD
  on both hosts UNLOAD was rejected with `unload is busy; active_owners=224(local node-3)/256(remote node-1)`,
  `requests=0`, and the drive exited. The `active_owners`/`active_frontiers` check was added to `worker/shutdown.rs`
  by `2e9451a5c`. **This observation cannot establish a "release leak after 512 completions".**
  When `run/inference.rs` receives ERROR it fills `failure`, leaves the loop and returns `Ok(InferenceResult{error})`,
  but the caller at `run/mod.rs:174` does not look at that `error`; it sends UNLOAD, and if receiving that response
  fails it exits immediately via `?`. So the first inference error and the partial results are lost, and the final message only
  shows UNLOAD busy. It cannot be ruled out that this was a normal busy rejection after inference had stopped.
  The producer of RELEASE is the worker, not the drive (the drive only consumes `RELEASE_RECEIPT_CONTENT_TYPE`), so
  the hypothesis "the drive sent the last RELEASE late" does not hold either. Re-judgement happens only after preservation of the first error and partial results
  is fixed. On the remote host, the 4 stages of the failed run remained on the agent, and the next run
  failed immediately with `node already exists` (`20260907T093529Z`).
- gemma-4-31B (dense, 60 layers, 36/24) VRAM-only 2-stage was accepted by the harness, and the stage log shows weights and KV
  all on `CUDA0` (host 314 MiB). However, **all 32** responses carry the `<|channel>thought\n<channel|>`
  marker in front, all 32 stopped with `length`, and 3 were truncated with an odd number of code fences.
  A pass from `judge.mjs` is the heuristic pass described in §4, not an approval of response completeness.
- Running 35B MoE expert-CPU offloading with mmap made `CPU_Mapped 11.8/11.4 GiB` load through page faults from the S: share,
  and it timed out after 30 minutes with 1 stage execution. The rerun with `--no-mmap` and a 60-minute timeout
  is §3.2 below.

### 3.2 RAM offloading arm (`--no-mmap`)

Remote 3090×2, HEAD Release binaries, launcher as above. Offloading uses the `--override-tensor` pattern
`blk\..*\.ffn_(up|down|gate)_exps.*=CPU` to keep and compute only the routed experts of the owned layers on the host; router, attention,
norm, embedding and KV are on the GPU. The split was confirmed from the buffer placement in the stage log.

| run | scenario | target | structure | meaning | acceptance | gen TPS | rows/s(legacy total) | physical batch | rows/batch | ms/batch | mixed | ubatch fill% | GPU util mean/p50/p90/zero/peak |
| --- | --- | --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| `20260907T102817Z-8438348c` | offload_35b_moe_2stage | remote 3090x2 | 64/64 | 64/64 | accepted | 43.19 | 63.57 | 2462 | 22.52 | 354.3 | 0 | 4.4 | GPU0 16.8%/p50 7/p90 49/zero 41%/peak 3798 MiB; GPU1 16.7%/p50 7/p90 51/zero 40%/peak 3798 MiB |
| `20260907T104752Z-006d6389` | offload_122b_moe_1stage | remote 3090x2 | FAILED | - | drive exited 1 | - | - | - | - | - | - | - |  |
| `20260907T104825Z-53c07be8` | offload_122b_moe_2stage | remote 3090x2 | FAILED | - | drive exited 1 | - | - | - | - | - | - | - | GPU0 0.5%/p50 0/p90 1/zero 77%/peak 1678 MiB; GPU1 0.2%/p50 0/p90 1/zero 84%/peak 1678 MiB |
| `20260907T105822Z-9888cd38` | offload_122b_moe_2stage | remote 3090x2 | FAILED | - | drive exited 1 | - | - | - | - | - | - | - | GPU0 0.3%/p50 0/p90 1/zero 81%/peak 4930 MiB; GPU1 0.3%/p50 0/p90 1/zero 80%/peak 4994 MiB |

- `offload_35b_moe_2stage` (Ornith-1.0-35B, 20/20 layers, ChatML, 32 seq, 64 requests): accepted, judge 64/64.
  Stage log: `CUDA0 model 1237.22/721.90 MiB`, `CUDA_Host model 10701.84/11115.15 MiB`, `CUDA0 KV 425 MiB`×2.
  Compared with the VRAM-only arm at the same cut and request count (117.07 gen TPS before the quality verdict, ms/batch 132.5), it reached 43.19
  with ms/batch 354.3, 2.7 times slower. Judge 64/64 is not a quality comparison with the 60/64 of the VRAM-only arm, which sends gemma turns,
  because the templates differ; it is the heuristic pass described in §4, not approval of meaning or completeness.
  Run-window GPU utilisation is 21.8/21.6%, the lowest on this ladder.
- `offload_122b_moe_1stage`: **cannot run**. `run/config.rs::validate` in `p4-event-drive` rejects fewer than 2 nodes
  (`20260907T104752Z-006d6389`). The scenario was removed.
- `offload_122b_moe_2stage` attempt 1 (`20260907T104825Z-53c07be8`, cut 25/24 over 49): node-0 loaded with
  `CUDA0 2417.37 MiB`, `CUDA_Host 40476.50 MiB`, but node-1
  exited with `llama-graph.cpp:1529 GGML_ASSERT(0 <= begin && begin < end && end <= n_layer)` (0xc0000409).
  The GGUF has `qwen35moe.block_count=49` and `nextn_predict_layers=1`, so the fork's `n_layer()=48`.
  The cut was corrected to 24/24 on a 48-layer basis and rerun.
- `offload_122b_moe_2stage` attempt 2: **BLOCKED** (`20260907T105822Z-9888cd38`, cut 24/24 over 48). Both stages loaded — node-0 `CUDA0 model 2210.30 MiB`, node-1 `CUDA0 model 2983.28 MiB`, `CUDA_Host model 38325.07/38996.04 MiB`, `CUDA0 KV 204 MiB`×2, compute `CUDA0 1122.01/1224.01 MiB`. However, node-1 exited with code 5 on `stage_memory_plan.cpp:358` `planned and actual memory differ at entry 1`: the compute of the host entry was 107,251,776 B planned versus 142,951,040 B actual (`sched_reserve: CUDA_Host compute buffer size = 136.33 MiB`), while the other fields (model 40,186,750,976 B, context 138,936,320 B) matched. On node-0, plan = actual. This is a staged server problem where the host compute buffer prediction is off when the tail stage keeps and computes experts on the host, and the server was not fixed in this session. Therefore **normal responses and TPS for 122B RAM offloading were not measured**; only loadability (about 39 GiB host per stage, about 3 GiB GPU per stage) was confirmed.

### 3.3 Harness and drive defects that obscure the measured verdicts (confirmed in the 2026-09-08 code review)

This section was confirmed after the measurements by re-checking the same artifacts against the source at HEAD `0681d1c38`. All three defects
are **reasons not to read the failure/acceptance marks in the tables above at face value**, so they are fixed before any re-judgement.

| Defect | Anchor | How confirmed | Impact |
| --- | --- | --- | --- |
| The first inference error and partial results are masked by the UNLOAD failure | the ERROR branch in `tools/event-drive/src/run/inference.rs` and the caller at `run/mod.rs:174` | Source comparison. The caller does not read `run.error`; it sends UNLOAD, and a failure receiving that response exits immediately via `?` | The failure cause of both `pressure` runs was left only as UNLOAD busy |
| A child killed by a signal is judged a stop failure | `test/benchmarks/p4-4node/run.mjs::stopChild` | Reproduced by applying the same shape to an independent child: exit event at 13 ms, `exitCode=null`, `signalCode='SIGTERM'` → returns false | `NOT_ACCEPTED(agent_stopped)` on the local arms is meaningless |
| Stages of a failed run remain on the remote agent | no UNLOAD/cleanup on the failure path of `run.mjs` | `20260907T093529Z-8d1cb2a8` failed immediately with `node already exists` | This ladder worked around it by restarting the agent for every scenario |

## 4. What this record does not prove

- Distribution across multiple physical computers (H6), H1's varied corpus and full-response judgement, H2's three modes cold/sustained/recovery,
  or H5's paired A/B. The arms in the tables above are single runs of the development harness.
- General deadlock freedom of the candidate. Only one cap1 test and 1 mutation kind were checked.
- A comparable performance improvement. The values only matched the 09-04 baseline with the same launcher, model and topology,
  and since this is not a paired A/B, not even non-regression is proven by the H5 standard.
- **Meaning and completeness of responses.** `judge.mjs::DEFAULT_CRITERIA` checks only 200 characters, Hangul ratio 0.3, 4 domain terms,
  12-character shingle repetition 0.35, absence of U+FFFD, and allowed stops. A short explanation with wrong facts, a response with leftover markers,
  and a response truncated by `length` can all pass. `meaning n/m` in this document is that heuristic count.
- **The cause of `pressure`.** The first inference error is masked and lost behind the UNLOAD failure, so the current artifacts cannot decide it.
- **Whether the agent stopped on the local arms.** Because `stopChild` misjudges signal termination, the `agent_stopped` value is not evidence.
