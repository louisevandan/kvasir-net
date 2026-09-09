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

## Shared compute buffer sizing

Mac rerun `20260909T212421997Z` passed buffer ownership but exposed a second, independent mismatch: planned host compute 768,237,568 bytes versus actual 384,118,784 bytes.
BLAS and CPU use the same physical compute buffer. `ggml_gallocr_get_buffer_size` counts that allocation only at its first index, while `ggml_gallocr_reserve_n_size` counted the same allocator at every index.
The equality gate correctly rejected LOAD and remains unchanged.

Patch `0027-noalloc-shared-buffer-size.patch` makes the no-alloc size report use the same allocation identity, before any real buffer exists.
The mandatory runtime compile test exercises the real allocator with one, two and three references to the CPU buffer type.
It verifies no tensor storage exists after measurement, compares every reported size with actual allocation, then allocates and computes 257 independently checked sums.
Before the fix, Windows measured index 1 as 2,112 bytes while actual was 0 (`reserve-size-red-consumer.log`).
The first test draft did not compile because it used an unavailable graph leaf-count accessor; that result is not the failure counterexample. The corrected test uses the two explicitly constructed leaves and then demonstrates the runtime mismatch.

An independent copied ggml tree on Mac passed the control and failed after removing duplicate detection (exit -6).
Recompiled `libggml-base` SHA256 before/after: `89dde2582f38b1d18f0784e5ff43aa791e23069723f9aecde8bc87cfb887d2e9` / `3af3b0278826adf2e60417245a28d89756d0909a6f0195ca5aa9026258216c21`.
Probe SHA256: `f396c8409a5ea3736614a7c068f985b4848d87bf76564b7c9ffa0ec0abf6b52f`; full commands and source hashes are in `reserve-size-mutation-result.log` and the independent build directory.

Current queue: 26 active patches, 4 upstream fixes / 18 stage hooks / 4 model features.
Patch-set SHA256: `ff1468f6187f0e70e2970f73c0d0ca489d7b8bd4465b583ec1b6305cbee56f3b`; patched tree: `44493ca7ab53699ecc012e915be10da5ad7c7b1f`.
Fresh replay, manifest/classification and private-header checks pass (83 files; no new private/common crossing).
Windows, Spark, Ubuntu and Mac `.20` pass 16/16 CTests on this queue; Mac `.21` receives the verified Mac runtime package before its next model rerun.
The Windows existing prepared directory was updated only after checking its full old patch digest, then verified with strict explicit `prepare-pipeline-upstream --out` against the new digest before building. Its old hash-bearing directory name is not its current identity.

The byte-preserving monolithic reference (`--binary-file`, same 64-token first prompt) reproduces the small model's Korean/formatting problems; the earlier newline-stripping reference is excluded. This observation does not establish numerical equivalence across batch shapes or certify the staged output quality.

## Current queue: individual model execution

These runs use native source `6c9c2894072c71d0f9093c092f387836af3f32f9` and the 26-patch digest above.
Each `source.json` includes the native executable, runtime libraries and agent file hashes captured before execution.
The Windows Rust binaries were built from the release checkout and Unix Rust binaries from `612b75496`; their Rust source is unchanged by the subsequent native fixes. They are not represented as binaries rebuilt from the final native commit.

| Run / arm | Model | Requests / completed / released | Inference elapsed | Inference / cleanup error |
| --- | --- | --- | --- | --- |
| `20260909T213934697Z-mac21` | Qwen2.5-1.5B Q8_0 | 8 / 8 / 8 | 540,134 ms | null / null |
| `20260909T214217374Z-local` | Gemma 4 E2B Q8_0 | 8 / 8 / 8 | 9,327 ms | null / null |
| `20260909T214217374Z-spark` | Gemma 4 E2B Q8_0 | 8 / 8 / 8 | 10,853 ms | null / null |
| `20260909T214217374Z-ubuntu` | Gemma 4 E2B Q8_0 | 8 / 8 / 8 | 10,299 ms | null / null |
| `20260909T215021079Z-mac21` | Gemma 4 E2B Q8_0 | 8 / 8 / 8 | 496,743 ms | null / null |
| `gemma-full-gpu-20260909T215934817Z-mac21` | Gemma, corrected GPU layer count | 8 / 8 / 8 | 165,639 ms | null / null |
| `gemma-matched-kv-20260909T220539322Z-mac21` | Gemma, K/V both f16 | 8 / 8 / 8 | 8,329 ms | null / null |

All seven arms report complete evidence and EOS for all requests. Mac's successful LOAD closes the two observed memory-plan failures for this arm; it does not certify the full model/backend matrix.
The Qwen answers retain the monolithic reference's incorrect Korean explanations and sentence-count failures.
Gemma's RAM/storage, 84-litre calculation and calibration answers were manually inspected and coherent; its wire-heating answers include imprecise friction analogies and an unqualified resistance/temperature feedback claim. Full quality acceptance is not granted from the harness's structural pass.

Gemma file SHA256 is `0a8488b149e1f700712c35d5bf0a3795f9dcc2563b4944d5ef2fb89375f9483e` (5,048,350,848 bytes).
Its embedded turn template is used with native BOS insertion and cuts `[0,13) [13,35)`.
Both models use two stages on one host, resident 4, two four-request waves 500 ms apart, maximum 512 output tokens, EOS/stop required and no accepted length termination.
These short, single-run elapsed times include inference prefill and queueing, exclude LOAD/UNLOAD, and are not a GPU performance ranking.

The first individual configurations used `n_gpu_layers = trunk_layers - layer_begin`.
Upstream includes the output layer (and auxiliary layers where present) in its offload indexing; native logs show the first owned layer remained on CPU.
The paired Mac placement arm changes only that argument to `--n-gpu-layers 999` (plus run identities), preserving explicit unowned-tensor CPU overrides. These are single runs with different generated token sequences, not a repeated throughput comparison. Earlier arms are not relabelled as entirely GPU-resident.
During the corrected arm, a two-second `sample` of its own stage process still finds CPU flash attention and CPU barriers. The selected Metal backend's `supports_op` rejects different K/V types; these plans used q8_0 K and f16 V.
The next arm changes only K from q8_0 to f16 (plus run identities). It completes all eight requests in 8,329 ms and both native stages report zero CPU compute allocation on cleanup. This attributes the observed fallback to the tested configuration; it is not a repeated performance benchmark or proof that every Metal model avoids CPU work. Its heating explanations still do not earn full quality approval.

## Three-host 122B preflight

Preflight `20260909T215244120Z` uses Spark `[0,32)`, Mac `.21` `[32,45)` and Ubuntu `[45,48)` for Qwen3.5-122B-A10B UD-Q5_K_S.
The MTP auxiliary layer is excluded (`spec-type none`). The model is three GGUF shards, 88,310,156,320 bytes in total; the vision projector is not part of this text-model identity.

| Shard | SHA256 |
| --- | --- |
| 00001 | `ea08ef11402edf53a98cb90329961ee3dbe191dcf75ff7c99f009264cf8a99b3` |
| 00002 | `6d4910dc603ab82570d2c8a1a979414e9d921af68273113063e7679424d53fa7` |
| 00003 | `e0f018ae1f3016fab56005ba4b25e708720eaa2413232bd65ba07c7c6d964938` |

All three actual native no-alloc plan consumers return `complete=true`, `fits_current_free=true`.
Required bytes are Spark device 57,181,003,904 + host 1,346,619,424; Mac device 23,301,334,048 + host 56,901,664; Ubuntu device 6,892,271,744 + host 14,960,672.
Spark and Mac charges share their respective physical host pools; their advertised host and device capacities are not added.
The Mac free-memory snapshot was taken during its separate small-model run, so admission must be checked again at actual LOAD.
All nine agent-port probes (three hosts to all three, including self) connect on 52004. This is TCP reachability, not yet event-ring or model inference acceptance.
`large-plan-*-result.json` binds the plan bytes and executable hashes; local full-shard hashes describe the shared NAS files, not independent full reads on every remote host.
The remaining targets and user-input gates are unchanged. A three-host run cannot close the requested all-computer gate.

Code inspection before the heterogeneous run finds another existing gate: `v2::build_identity::agree` requires identical backend inventories. CUDA and Metal are deliberately rejected even at the same upstream/patch source. Its existing negative tests are preserved; no identity is forged and no allow-unidentified override is used.
The physical v4 capsule carries raw engine tensor type values and bytes. Heterogeneous execution needs an explicit codec/representation compatibility contract and consumer validation; dropping the backend comparison alone is not sufficient.
An independent two-CUDA-host diagnostic uses Spark `[0,45)` and Ubuntu `[45,48)` while that contract is examined. Spark's actual no-alloc plan requires device 80,361,451,648 + host 1,346,619,424 bytes and passes its shared physical pool check. It is not a substitute for the requested whole fleet.

All repository Node test files on the current native source were rerun: 159 passed, 0 failed, 0 skipped (`node-tests-current.log`).

## Two physical hosts: 122B execution

`large-2-hosts-20260909T220205942Z` completed 8 / 8 / 8 requested/completed/released, all EOS, with null inference error, cleanup error and missing evidence.
It uses the sealed `6c9c28940` runtime identities, Spark `[0,45)` and Ubuntu `[45,48)`, resident 4 and two waves of four 500 ms apart.
The verified embedded ChatML template uses the model's no-thinking generation prefix; MTP remains disabled.
Inference elapsed is 30,353 ms, excluding LOAD and UNLOAD; 846 emitted outcome tokens include eight EOS outcomes, while decode rows total 838. These are not interchangeable TPS numerators.

Both stages' model/context/compute byte totals match PLAN and ACTUAL.
The startup-to-cleanup monitor captured 712 Spark and 702 Ubuntu samples. Ubuntu's global GPU usage peaked at 6,711 MiB.
Spark's GPU memory query reports unavailable for its shared pool; it is not replaced with a fabricated VRAM peak. Minimum system MemAvailable was 41,620,528 KiB while MemFree fell to 704,636 KiB during NAS caching; neither metric is a per-process allocation measurement.
The monitor includes other host activity and startup time, so its utilization samples are not a steady inference utilization result.
ACTUAL's `fits_current_free=false` compares the allocation with the already-reduced remaining free memory; admission used the pre-allocation PLAN and actual allocation equality passed.

Manual review read all eight complete responses: RAM volatility/persistence, the 84-litre calculation and reference-based sensor calibration are coherent and follow the requested format. Wire-heating responses still use an imprecise friction analogy; blanket scientific-quality approval is withheld.
This closes a bounded two-host 122B execution path, not all-fleet participation, sustained pressure, service acceptance or performance optimization.
`summary.json`, full artifacts/configuration/runtime hashes, both agent logs and memory samples are retained in the run directory.

## Heterogeneous wire candidate (not yet runtime-approved)

The separate `codex/fleet-wire-compat-20260910` checkout preserves the completed arms above.
Its opt-in `physical-wire-v4` contract is specified in the adapter restructure document; default exact-build rejection and the old negative tests remain.
The HELLO-to-LOADED-to-OUTER path carries `stage_wire_abi`, and artifacts keep every stage identity rather than reporting only the head's backend inventory as the whole pipeline.
The actual LOAD reply consumer exercises reversed reply arrival, explicit opt-in, missing identities and changed wire source; native execution and peer processes are not simulated by that fixture.

Initial workspace rerun ended 1,377 / 1 / 7 solely because the new contract paragraph mixed line endings. After normalizing that document, the complete run ended 1,378 / 0 / 7 across 58 summaries.
Current native builds on Mac `.20`, Spark and Ubuntu pass 16/16 CTests, including the compiled wire-identity assertion.
Their source fingerprint and full runtime type-size tables match byte-for-byte: source `e691ace7f2c7eef550c8c8c6b455b1de9038307819baf39eb7a939038ae8abd5`.
Windows native compilation, independent mutations and actual heterogeneous model execution remain pending at this checkpoint. No three-host acceptance is claimed.

## Local evidence and reproduction

Raw discovery, NAS inventories, compiler versions, conflicts and rebase logs are under `F:/dev/p4/target/fleet-20260910`.
These ignored files are local evidence, not a portable publication bundle.

```powershell
node layers/adapters/llamacpp/staged/scripts/prepare-pipeline-upstream.mjs --json --out F:/dev/p4-fleet-20260910/target/prepared-latest
node layers/adapters/llamacpp/staged/scripts/build-stage-server.mjs --cuda --cuda-architectures '86;89' --generator Ninja --config Release --parallel 8 --build-dir F:/dev/p4-fleet-20260910/target/native-latest-cuda
```

The upstream checkout must be clean at the full candidate SHA before preparation. The release checkout and its prepared source are not inputs to these commands.
