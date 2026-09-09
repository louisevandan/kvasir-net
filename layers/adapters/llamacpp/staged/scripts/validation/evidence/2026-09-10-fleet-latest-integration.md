# 2026-09-10 fleet access and latest upstream integration

Status: WIP. The execution order belongs to the [roadmap](../../../../../../../../docs/distributed-batching-roadmap.md).
This record does not approve a release, a model, or a distributed wave.

## Source boundary

- Release candidate remains at P4 `40d91d025bc2fa0dd0167c9710ef7c9d74017723` in `F:/dev/p4`.
- Development uses `F:/dev/p4-fleet-20260910`, branch `codex/fleet-latest-20260910`, initially from the same commit.
- Official upstream fetched from `https://github.com/ggml-org/llama.cpp.git`: `434ddbbc0e30522e897670681e503b797c12b7c1`, `ci: fix sanitizer tests (#28583)`, nearest tag `b10883`.
- Compared with `0eadefebd3f8f92a86d634a0e5b8fffc9dc792c0`: 435 files, 47,513 insertions, 8,347 deletions. Latest means the observed fetch, not an immutable claim about future master.
- Adapter policy, ledger and P4 transport source changes: zero at this checkpoint.

## Current access observations

These are live observations through SSH and filesystem reads, not historical deployment claims.
Memory totals are not allocation budgets; running applications, OS, backend scratch, KV and transport buffers must be subtracted at LOAD.

| Host | Current hardware observation | NAS model access | Runtime status |
| --- | --- | --- | --- |
| This PC, `.6` | RTX 3090 24 GiB + RTX 4080 16 GiB; 255.9 GiB host RAM | `S:` maps to `//192.168.0.13/file-station` | CUDA Release and 16 CTests pass; individual inference pending |
| Spark, `.26` | GB10; 121.6 GiB shared system memory | `/mnt/file-station/models`, 156 GGUF files; CIFS source verified | SSH/CUDA 13.0/Cargo available |
| Mac mini, `.20` | M4 Pro, 64 GiB unified memory | Existing placeholder contains zero models; no SMB mount | SSH/Metal tools/Cargo available; NAS authentication needs user connection |
| Mac mini, `.21` | M4 Pro, 64 GiB unified memory | `/Volumes/file-station/models`, 156 GGUF files | SSH/Metal tools/Cargo available; existing LM Studio left running |
| Ubuntu laptop, `.19` | RTX 2070 Max-Q 8 GiB; about 30.7 GiB host RAM | `/mnt/nas-file-station/models`, 156 GGUF files; path resolves into the logged-in user's GVFS SMB mount | User-scope Git 2.53.0 and Rust 1.97.1 installed; CUDA Release and 16 CTests pass |
| TUF laptop, `.17` | Not verified in this run | Not verified | TCP 22 reachable; SSH key rejected; access information requested |
| M42-SERVER2, `.29` | Prior release evidence: RTX 3090 x2; refresh before new LOAD | Interactive `42mob` login/S: recovery pending | SSH restored after planned Windows Update reboot; release agent/stage remain stopped |

Spark and Mac GPU memory are the same physical pool as system memory. They must not be added a second time.
The `.21` working repository is dirty and was not changed. New builds use isolated directories.

Port probes found 19001 listening only on this PC among the hosts tested. Its health response identifies an existing Linker service; it is not a free P4 port.
Local Windows firewall inspection found a TCP 52001-52005 rule. Both Mac application firewalls report disabled.
Linux firewall inspection requires root and remains unverified. A closed TCP probe does not distinguish a firewall from an absent listener.
Peer-to-peer connections and actual P4 delivery remain to be tested.

Follow-up port checks: Windows excludes 51952-52051, so the allowed 52004 cannot bind (WSA10013).
51054 binds and has an existing allow rule, but an explicit inbound block for this run's `p4-agent.exe` overrides it.
Spark-to-Windows connection times out; administrator removal of that program block was requested. No rule, exclusion or unrelated service was changed.
Spark, Ubuntu and Mac `.21` listen on 52004 and passed Spark-origin TCP connection probes; protocol/model acceptance is separate.

Mac `.20` normal SMB mount and NetFS NoUI attempts failed authentication. The matching NAS Keychain item exists, but Security.framework returned `-25293` when asked to use it without user interaction. No password was logged or transferred. User connection through Finder was requested.

## Compatibility replay

Plain replay of the old 26-patch queue failed on 0001, 0015, 0016, 0018 and 0020.
The candidate queue has 25 patches and replays on a fresh official tree; `prepare-pipeline-upstream.mjs` passed its patch, runtime-boundary and prepared-tree checks.

| Identity | Value |
| --- | --- |
| Candidate patch set SHA256 | `a22eed7eea2c49ea30f97fc353193d1eb3c24b7e6461514619ed6c3b73103675` |
| Candidate patched tree | `d07413951cb24a696324f1fe34483a7887573460` |

0001 is retired in this candidate: upstream `992cb503cdacf691ef06c332d05243bc7807257b` removed the old split-capacity check and uses dynamically growing split inputs. The real cross-backend boundary is protected by the conformance test below; full model/backend acceptance remains separate.

0020's remaining rejected hunk was the recurrent rollback test context helper. Upstream added a `fill` parameter and poisoned-buffer/nonfinite-logit checks. The port retains those checks, adds `n_ubatch` as a fourth parameter, and passes `fill` into the added regression contexts. No expected result or tolerance was relaxed. The runtime regression has not yet run against this candidate.

Individual normal responses, model-specific conformance, large model cuts/placement, all-host distributed delivery and sustained waves remain pending. Replay success alone is not update success.

## Native portability and input-growth conformance

The first full builds exposed three P4 portability defects, preserved in the raw build logs:

| Failure | Fix | Scope |
| --- | --- | --- |
| Mac: unconditional `process.h` in runtime compile test | `_WIN32` PID branch and POSIX `getpid()` | Keeps the real KV restore test available on each OS |
| Linux: `std::any_of` undeclared in `llama_stage_runtime_kv.cpp` | Include the owning `<algorithm>` header | No runtime behavior change |
| GNU linker: undefined `common_*` references from `p4_llama_compat` in two test executables | Declare `llama-common` as the facade's private link dependency | Correct dependency order; no global linker relaxation or new public dependency |

Windows CUDA at `612b75496` built and passed the pre-existing 15 CTests. The portability candidate passed 16/16 CTests on Windows CUDA 13.1/sm_86+89, M4 Pro Metal, GB10 CUDA 13.0/sm_121 and RTX 2070 Max-Q CUDA 12.4/sm_75. The second Mac passed the same 16 tests using the verified binary archive. Rust agent and drive Release builds succeeded on all three Unix architectures. Optional model tests inside existing CTests still require explicit model runs; these counts do not certify those skipped sections.

`p4_staged_split_inputs_test` uses the real ggml scheduler, CPU-owned input buffers that the GPU cannot consume directly, and a graph assigned to the accelerator. It checks 29/30/31 and 59/60/61 unique cross-backend inputs, including one operation adding two new inputs across a capacity boundary. Every case must execute as one GPU split, produce the independently calculated sum in all 128 elements, and preserve every input value. Dedicated and integrated GPU types are both eligible; absence of a GPU fails the probe. CUDA/Metal CTest configurations run it; CPU-only builds compile it but require an accelerator to execute it.

The initial standalone probe's implicit graph allocation cleared manual backend assignments and computed on CPU. The explicit GPU placement assertion caught that error. The corrected probe calls `ggml_backend_sched_alloc_graph` before compute; its trace shows 29 through 61 inputs on `MTL0`. That failed probe is not counted as GPU evidence.

Mutation uses a separate copied ggml tree and build under Mac `.20`'s `split-growth-mutation`. It preserves initial allocation but rejects subsequent input-array growth. Final probe source SHA256 is `f920f0e30c8ab8d47c7de48204b1b1cf64123c05604291ff25bcb10d38bba410`.

| Mutation identity | Value |
| --- | --- |
| Independent control | exit 0 |
| Growth removed | exit -6 / assertion failure |
| `ggml-backend.cpp` before / after SHA256 | `dce5259cbfb60ff3ea8a42b382ecc530dce82760deee2f65898c167c9465a877` / `ef9a7b5d5cba902a24bb7f4ad4fc199b526e6038df80a481074343a42c232c6b` |
| Recompiled `libggml-base` before / after SHA256 | `53710fb0d0b2acc27eb63cf1993326dfd924eb644b796b7f8f49975276eec8bf` / `18debc4d6a50d1674820a5a414993b849c9af626ac7c2dd91fafe9cd95030e10` |
| Final standalone probe SHA256 | `5de8d3310b1c7c4d579a2870a181a7dc3c6e94bffee80f46fd5fda123e466fc4` |

The live P4 prepared tree and runtime libraries were not mutated. Native source updates were sent with normalized before/after file hashes because Git archive source line endings differed from the local worktree patch context. All before hashes were checked before writing the candidate. Per-host `portability-source*.json` records bind those updates; no failed test or input was removed.

## Saved-plan loading option compatibility

First individual run `20260909T204558394Z` preserved four failed configurations and driver logs.
Windows failed to connect to its excluded port; Spark, Ubuntu and Mac `.21` reached native startup but exited before inference submission.
A length-prefixed `--validate-plan` probe isolated `invalid argument: --no-mmap`: latest upstream removed that deprecated CLI alias.
There was no submitted inference to assemble into a partial artifact in these runs.

`LlamaPlan::parse_arguments` now translates removed loading flags inside the existing compatibility facade.
It reads upstream option arities before scanning, preserves option-looking values and CLI ordering, and retains the Windows synthetic-argc guard after expansion.
Old pins that already register the aliases bypass translation. This change covers saved CLI plans, not restoration of removed environment-variable aliases.
The 25 upstream patches and their digest are unchanged; an initial common/ patch draft was discarded because it did not fit the declared patch classes. No classification allowance was widened.

The request-options CTest now consumes saved flags through `parse_llama_options` before its optional model section.
Cases cover positive/negative aliases, underscore normalization, canonical/legacy precedence, literal values and invalid/missing options.
Its six process arguments exercise Windows argv ownership after a one-token alias expands to two tokens.
The initial counterexample failed in `load-mode-red.log`; final Windows, Spark, Mac `.20` and Ubuntu builds pass 16/16 CTests.
In an independent Mac source/object/archive/executable copy, the control exits 0; removing alias translation or value arity each exits -6.
Each variant was compiled and linked independently. `load-mode-mutation-result.log` records commands and all four source/object/library/binary SHA256 values; the live runtime was not mutated.

The separate manifest validator initially rejected retired slot 0001 as an unexplained gap.
It now requires each skipped slot to have an explicit upstream commit and reason, while retaining ordering, overlap, missing-slot and active patch byte checks.
The retired commit is an ancestor of the selected pin (`git merge-base --is-ancestor`, exit 0).
Manifest/classification tests pass 13/13; classification remains 25 patches: 3 upstream fixes, 18 stage hooks, 4 model features.
The full Rust workspace finished with 58 summaries, 1,374 passed / 0 failed / 7 ignored; Rust code is unchanged by these native compatibility fixes.

## Individual execution and host buffer ownership

Run `20260909T211155072Z` used source `00e9aedb482166049487c3467322a7a98d601feb`, the unchanged 25-patch digest above, and NAS Qwen2.5-1.5B-Instruct-Q8_0 (`7185d306cf45956c8c017cd0d3b05ecc6bc18b3ea8eb5c240dce40e87563db7f`, 1,646,573,312 bytes).
Each arm uses two stages on one physical host, four resident sequences and two waves of four requests.

| Arm | Requested / completed / released | Inference / cleanup error | Scope |
| --- | --- | --- | --- |
| This PC | 8 / 8 / 8 | null / null | CUDA execution and cleanup pass |
| Spark | 8 / 8 / 8 | null / null | CUDA execution and cleanup pass |
| Ubuntu | 8 / 8 / 8 | null / null | CUDA execution and cleanup pass |
| Mac `.21` | No inference submitted | Native memory-plan rejection | CPU_REPACK was treated as an unknown accelerator allocation |

All 24 completed requests ended at EOS, but the harness only checks stop reason, nonempty response and replacement characters.
Manual review found bad Korean explanations and failure to follow three-sentence requests. The pump calculations correctly return 84 litres.
These are execution passes, not normal-response quality acceptance, performance rankings or fleet approval.
A monolithic completion reference is being checked with identical prompt bytes; the initial `--file` probe removed the trailing newline and is excluded from equivalence claims.

The Mac failure is physical ownership classification: `CPU_REPACK` deliberately reports `is_host=false` for its tensor representation, while its owning device is CPU.
`stage_buffer_uses_host_memory` accounts such buffers to host RAM. An unrecognized non-CPU device still fails, now with buffer/device names.
The generic backend probe enumerates real extra CPU buffer types, and the actual no-alloc model consumer is checked separately.
Final native CTests pass 16/16 on Windows, Spark, Ubuntu and both Macs. Shared topology still adds host and device allocations against one shared physical pool.

An isolated Mac model-plan mutation recompiles `stage_memory_plan.cpp`, replaces that object in a copied runtime archive and links a separate server.
The control passes; removing CPU ownership recognition exits 7 with `buffer=CPU_REPACK device=CPU`.
Control/mutant binary SHA256: `9395a408a266943a9b4e1ebde25fea7ae9c5afe21985fc8c0d6932b3189e324c` / `19458c263d7efc84c0c05fe45e326c9fa212c0449a9960e4d687d337aea41db0`.
Full source/object/archive hashes and commands are in `host-memory-mutation-result.log`.
That diagnostic used a hash-verified model copy on Mac `.20`; it does not establish that Mac's NAS connection.

## Local evidence and reproduction

Raw discovery, NAS inventories, compiler versions, conflicts and rebase logs are under `F:/dev/p4/target/fleet-20260910`.
These ignored files are local evidence, not a portable publication bundle.

```powershell
node layers/adapters/llamacpp/staged/scripts/prepare-pipeline-upstream.mjs --json --out F:/dev/p4-fleet-20260910/target/prepared-latest
node layers/adapters/llamacpp/staged/scripts/build-stage-server.mjs --cuda --cuda-architectures '86;89' --generator Ninja --config Release --parallel 8 --build-dir F:/dev/p4-fleet-20260910/target/native-latest-cuda
```

The upstream checkout must be clean at the full candidate SHA before preparation. The release checkout and its prepared source are not inputs to these commands.
