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
The final checkpoint below supersedes the build/mutation pending state. Mixed CUDA/Metal execution remains blocked before inference; no three-host acceptance is claimed.

## Final wire checkpoint and external blockers

Validated product source is `9ad366f9063e49f2574639d649fe556344992c6e`; later closure edits are documentation and archive manifests only.
The final complete workspace exits 0 with 1,378 passed / 0 failed / 7 ignored across 58 summaries. Node exits 0 with 159 passed / 0 failed / 0 skipped.
Native CTest is 16/16 on Windows, Spark, Ubuntu and both Macs; private headers 83 clean, manifest/classification 26 valid, docs-lint 92 clean.

The independent mutation checkout uses a separate build directory, requires a non-fresh compiler artifact and records source/test-binary SHA256 for every rebuild.
The actual LOAD consumer passes the control (exit 0) and rejects all four mutants (each exit 101): force the old exact profile, remove wire equality, remove admission, or drop the wire field during payload consumption.
No negative input, default rejection, global budget or judge was relaxed. `wire-mutations.json` and each build/test log retain the binding.

The first three-host attempt `wire-gemma-3-hosts-20260909T224106258Z` never reached LOAD.
Mac .21 received CREATE but its outgoing response was dropped by the kernel with NECP/error 65; the agent logged peer delivery failure. The run was explicitly stopped before any native model or inference submission.
The 9/9 Python socket checks did not prove that this process had OS network permission. No alternative executable path, relay or policy bypass was used.
An immediate two-CUDA retry exposed the retained CREATE nodes (`node already exists`); its failure is preserved and owned agents were restarted before retrying.

Run `wire-gemma-2-hosts-20260909T224459830Z` then correctly rejected Ubuntu's `upstream=unknown` before inference.
The diagnostic clone-build script omitted the user-local Git PATH. Ubuntu was rebuilt into `target/native-identified` after independently verifying HEAD and the actual patched working tree through a temporary index; the measured old build was kept.
Verified upstream is `434ddbbc0e30522e897670681e503b797c12b7c1` and tree is `44493ca7ab53699ecc012e915be10da5ad7c7b1f`. New server SHA256 is `83e3107e3adf098448eb3d937d7687f54d889fc76ca295db373fd8362726af2a`.
Rebuild and 16/16 CTests pass; no unidentified-build override was used. A LOAD rejection before submission still requires owned-process cleanup in this diagnostic harness; post-submission partial-artifact guarantees do not cover setup failures.

| Final arm | Physical hosts / backend | Requested / completed / released | Inference elapsed | Result |
| --- | --- | --- | --- | --- |
| `wire-gemma-2-hosts-20260909T225142318Z` | Spark + Ubuntu / CUDA | 8 / 8 / 8 | 7,314 ms | All EOS, error/cleanup/missing null |
| `wire-gemma-local-20260909T225459493Z-mac21` | One Mac, two Metal stages | 8 / 8 / 8 | 7,148 ms | All EOS, error/cleanup/missing null; no peer-connection proof |
| `wire-large-wave8-2-hosts-20260909T225349569Z` | Spark + Ubuntu / CUDA, 122B | 32 / 32 / 32 | 115,422 ms | All EOS, error/cleanup/missing null |

The final 122B arm keeps resident 4, cuts [0,45)/[45,48), eight waves of four at 500 ms intervals, maximum 512 tokens and the original four prompts repeated eight times.
It emits 3,360 outcome tokens including 32 EOS outcomes and records 3,328 decode rows; these are different numerators. The elapsed denominator excludes LOAD/UNLOAD.
All 32 incarnations are unique; physical slots 0/1/2/3 are released 9/9/7/7 times. Both monitors end with code 0 and stop_code 0.
TTFT p50/p90 is 47,404.5/88,278.1 ms; completion latency p50/p90 is 60,539/101,277.5 ms. Quantiles use linear interpolation at rank (n-1)q.
All 32 responses were read. RAM/storage, the 84-litre calculation and calibration answers are coherent; heating repeatedly equates resistance with mechanical friction. Blanket scientific-quality/service acceptance is withheld, and no failed prompt was removed.

Both stages' host/device model, context and compute byte totals match PLAN and ACTUAL. Ubuntu global VRAM peak is 6,711 MiB; Spark shared-pool GPU memory remains unavailable rather than zero.
Within each host's own stage RPC window, Spark has 109 GPU samples averaging 83.917%, Ubuntu 110 averaging 13.482%, neither with a zero sample. This is kernel-active sampling, not SM occupancy, absence of all idle intervals or a saturation claim.
Startup-through-cleanup memory records contain 790/780 samples. Ubuntu's process-path filter still names `native` while its rebuilt server is under `native-identified`, so **Ubuntu process RSS is missing**, not zero; GPU/system memory samples remain valid.
During LOAD, actual process maps on each CUDA host contain seven owned runtime files whose hashes match the pre-run identities. External OS/driver paths are listed but not represented as hashed P4 binaries.

A normalized comparison of 834 non-Markdown source files matches Mac .21 completely. Spark/Ubuntu differ only in the final additional LOAD consumer test case; their product sources match the validated commit. The complete final test file is exercised by the local workspace and independent mutations.

Current external gates are Mac .21 agent network policy, this PC's explicit inbound program blocks, TUF SSH account/key, Mac .20 SMB authentication and M42's interactive login/S: restoration.
These do not invalidate the bounded passes above and cannot be silently converted into whole-fleet acceptance. The next runtime action after access restoration is the same three-host Gemma gate, then 122B and all requested hosts; B2/B3, quality and sustained-service gates remain separate.
All task-owned agents/stages and monitors were stopped. Unrelated applications, old runtimes and the three unchanged v0.9.0 candidate archives are preserved. No stable tag or push was made.

## Candidate preservation

`F:/dev/p4-releases/fleet-20260910-wire-candidate` holds both source archives, four platform runtime archives, status/README, and the complete selected evidence archive (329 files, per-file SHA256 manifest).
[Candidate manifest](bundles/fleet-20260910/candidate-manifest.json) binds the archive hashes. Windows archive members and all three Unix archive manifests were read back and hash-checked.
These are preserved runtime candidates in their measured environments, not a claim of installation compatibility at arbitrary paths or drivers. Models and OS drivers are not bundled.
The old ignored target files remain available, but the selected evidence no longer depends solely on that build directory.

## Local evidence and reproduction

Raw discovery, NAS inventories, compiler versions, conflicts and rebase logs are under `F:/dev/p4/target/fleet-20260910`.
The selected source/runtime/evidence files are also preserved in the candidate bundle above; this is local delivery, not remote publication.

```powershell
node layers/adapters/llamacpp/staged/scripts/prepare-pipeline-upstream.mjs --json --out F:/dev/p4-fleet-20260910/target/prepared-latest
node layers/adapters/llamacpp/staged/scripts/build-stage-server.mjs --cuda --cuda-architectures '86;89' --generator Ninja --config Release --parallel 8 --build-dir F:/dev/p4-fleet-20260910/target/native-latest-cuda
```

The upstream checkout must be clean at the full candidate SHA before preparation. The release checkout and its prepared source are not inputs to these commands.

## Requested M42 + Spark + Mac run: 2026-09-10 recheck

The user requested 3090 x2, Spark and Mac mini together. This is a new three-physical-host target, not the earlier Spark/Ubuntu result.
Runtime source remains `9ad366f90`; this recheck changes no product source or acceptance criteria.
[Status and scope](bundles/three-platform-20260910/status.json), [per-file hashes](bundles/three-platform-20260910/MANIFEST.sha256), and [pending 122B configuration](bundles/three-platform-20260910/large-config.pending.json) preserve the attempt.

- Mixed CUDA/Metal prerequisite `mixed-1789013818305`: Mac received CREATE from Spark but its response failed. Kernel logs bind PID 76328 to an outgoing TCP drop with reason NECP and error 65. No model or inference was submitted. The driver exited 1 with event timeout while awaiting the CREATE response; absence of artifact.json is a pre-submission failure, not lost inference output.
- M42: SSH works and both RTX 3090 cards report 24,576 MiB total / 51 MiB used. There is no interactive user; S: and the NAS UNC are unreadable. Both enabled inbound Block rules name the existing `C:/Users/42mob/p4-remote/p4-agent.exe`. No runtime deployment, firewall change or model copy was performed in this recheck.
- Candidate 122B cuts are M42 GPU0 [0,8), M42 GPU1 [8,16), Spark [16,40), Mac [40,48), resident 4, unchanged 32 prompts / eight waves / max512 / no MTP. M42 plans and all actual LOAD/ACTUAL comparisons remain pending. The Windows executable path in the configuration is a proposed deployment destination, not an installed file.
- Actual native no-alloc preflight passes on Spark and Mac for those cuts, exit 0 and fits_current_free=true. Spark device 42,918,623,360 B + host 14,958,624 B; Mac device 15,708,870,656 B + host 14,698,528 B. These are allocations within each shared physical pool, not separate capacities to add. They are not peak measurements or inference acceptance.
- Both newly started agents were hash-bound, their logs copied, and they were stopped with zero stage children. Existing apps and sealed runtime/evidence bundles were preserved.

The next step needs Mac app network authorization and M42 login/NAS restoration plus scoped agent network configuration. Then deploy/hash-check the sealed Windows runtime, execute all four memory plans, validate the mixed-host small-model path, and run the exact three-host 122B waves with complete release, UNLOAD and manual response review. Do not substitute a subset of hosts or claim success from the two plan passes.
## Scheduled all-computer retry: 2026-09-10

The requested 15-minute heartbeat was dispatched at 14:03 KST and paused after dispatch to avoid another overlapping test.
Runtime product source remains `9ad366f9063e49f2574639d649fe556344992c6e`; driver SHA256 is `c96cf7539af6580c3cc610939f1981477928aaa7415bc1c512e11172c097ed23`.
The main checkout stays at `40d91d025`, wire checkout at `0c27873ef`, both clean. This retry changes no product source, builds, acceptance thresholds or test expectations.
Prior workspace/CTest totals above are not rerun results for this retry. No stable tag or push is made.

### Host recheck and setup

| Host | Actual result in this retry |
| --- | --- |
| M42 .29 / two RTX3090 | Interactive 42mob login and S: restored; separate sealed runtime deployed under releases/fleet-wire-9ad366f90/windows-runtime; all 13 manifest files verified |
| Spark .26 / GB10 CUDA | NAS read, P4 CREATE/DELETE replies, small and 122B inference |
| This PC .6 / RTX3090 CUDA | TCP 52004 is in Windows excluded range 51952–52051; agent uses 51054, actual CREATE/DELETE and inference pass; RTX4080 belongs to background apps |
| Ubuntu .19 / RTX2070 Max-Q CUDA | NAS read, actual CREATE/DELETE, small and 122B inference; server uses native-identified |
| Mac .21 / Metal candidate | Incoming CREATE reaches local-Codex-owned PID78595; fresh 14:31:28 KST kernel outgoing drop is NECP/error65; no model submitted on this host |
| Mac .20 | SSH succeeds; no SMB mount or P4 agent observed; no inference |
| TUF .17 | TCP/SSH endpoint reachable but admin authentication denied; agent probe timed out; backend/hardware inference not verified |

The candidate M42 firewall rule names only the deployed agent, TCP52004, and peers .6/.17/.19/.20/.21/.26. Existing blocks and applications are preserved.
The local attempt to add a firewall rule was denied; no local rule change succeeded. Actual P4 traffic succeeds at 51054 under existing policy.
The Mac process belongs to the Mac-local Codex task and was neither stopped nor bypassed. GUI toggles and generic socket success remain insufficient evidence of that executable's outgoing permission.
Failed CREATE probes may retain empty nodes on that Mac because their reply was unavailable; their deletion is not claimed. No Mac native model was loaded.

### Every inference arm

| Run | Physical hosts/stages | Requested/completed/released | Result |
| --- | --- | --- | --- |
| `gemma-four-cuda-hosts-1789017031156` | M42/Spark/local/Ubuntu, 4 CUDA stages | 8/0/0 | Exit1; overall inference deadline; missing requests8/stage_executions3; UNLOAD busy with pending4/flight1/owners4; partial artifact preserved |
| `gemma-four-cuda-hosts-1789017748328` | Same four hosts and four cuts | 8/8/8 | Exit0, all EOS, no inference/cleanup/missing errors, slots0–3 each reused twice; functional setup smoke |
| `122b-four-cuda-hosts-1789017057291` | Same four hosts, 5 CUDA stages | 32/32/32 | Exit0, all EOS, no inference/cleanup/missing errors; full release and idle UNLOAD |

The failed small run followed individual peer restarts and records a write failure. The transport caches outbound sender queues; enqueue success is not write completion, and a failed write is not automatically replayed.
All central-owned agents were stopped, failure logs retained, and all four restarted before the next small run. This is a setup recovery, not a source repair or reconnect/durability acceptance.
No agent restart occurs between the passing small and large runs. Initial failed-run cleanup forcibly stopped an owned M42 stage; this is not successful UNLOAD.
The firewall command-return marker is 05:23:30.551Z and small-run end is 05:23:26.718Z; the actual rule-mutation instant is not recorded. Its order relative to completion is unresolved, so the small run is not a sealed performance arm.
Large-run startup is after the rule update. A Mac CREATE probe was performed during large-run setup; configuration and product binaries were unchanged.

The large run uses Qwen3.5-122B-A10B UD-Q5_K_S, 88,310,156,320 bytes in three NAS shards, resident4, ctx8192, batch/ubatch512, unified KV Kq8/Vf16, max512, temperature0/seed7 and no speculative/MTP execution.
The original four prompts are repeated eight times, four arrivals every 500ms, cuts M42 GPU0 [0,8), GPU1 [8,16), Spark [16,37), local3090 [37,45), Ubuntu [45,48).
Driver wall interval is 05:24:27.881Z–05:36:43.011Z; inference elapsed is 220,856ms and excludes LOAD/UNLOAD. It has 3,394 sampled outcomes including 32 EOS and 3,362 decode rows: 15.2226 decode row/s, not quality-approved effective TPS.
TTFT p50/p90 is 83,673/167,304.4ms; completion latency p50/p90 is 111,142/192,643.1ms, quantiles at linear rank (n-1)q.
All 32 incarnations are unique; physical slots0/1/2/3 release 9/8/7/8 times. Every full response, prompt, token position and terminal stop remains in artifact.json; responses.jsonl and digest-bound judge.json support manual review.
All 32 responses were read. Heating explanations in requests1/5/9/13/17/21/25/29 equate resistance with mechanical friction and rubbing hands. RAM/storage distinctions, 84-litre arithmetic and calibration-purpose answers are coherent for these prompts; blanket scientific-quality and normal-service approval remain withheld.
The small run also retains two heating concerns. No prompt, failed output or limit was removed. This is not a controlled comparison with earlier two-host throughput and is not an optimization result.

### Memory, runtime and telemetry boundaries

Topology, execution shape, and all host/device model/context/compute totals match PLAN and ACTUAL for all five stages. The comparison excludes current free memory and fits_current_free, which observe different allocation states.
M42 GPU0/GPU1 and local3090 each require device14,393,862,272B; Spark37,574,310,016B; Ubuntu6,892,271,744B. M42 head host1,346,619,424B, other non-tail hosts14,958,624B, Ubuntu host14,960,672B.
Spark shares one physical host/device pool; reported host and device capacities must not be added. The preflight correctly refused Ubuntu while a previous Gemma model remained resident, then passed after unload. The mistaken local CUDA-order preflight and corrected RTX3090 result are both preserved.
During LOAD, mapped runtime files match sealed identities: Spark7, Ubuntu7, local9, each M42 stage9. Stage build identities retain upstream, patch and wire ABI; external OS/driver libraries are listed or outside this P4-owned hash check.
Model hashes in source.json reference the earlier NAS content audit; all 88GB were not rehashed during this retry.

| Device | Samples in its host enclosing RPC window | Mean kernel-active utilization | Zero samples | Global VRAM peak, startup through cleanup |
| --- | --- | --- | --- | --- |
| M42 GPU0 / GPU1 | 189 each | 20.228% / 20.619% | 43 / 39 | 14,581MiB each |
| Spark GB10 | 212 | 21.788% | 57 | Unavailable |
| Local RTX3090 | 199 | 22.930% | 49 | 14,271MiB |
| Ubuntu RTX2070 Max-Q | 208 | 6.962% | 1 | 6,711MiB |

These are per-host enclosing stage RPC windows using that host's timestamps, not synchronized cross-host overlap, SM occupancy or pure GPU-compute spans. Hosts have clock offsets.
The local RTX4080 remains background activity, excluded from participating-device averages. Windows native RSS is missing; Unix native RSS filter is corrected to include native-identified.
Four memory monitors exit0. Final samples show Unix native children absent; GPU usage has fallen after unload. A preset memory-return tolerance and sustained H7 baseline were not tested.
TCP snapshots bind actual agent processes to cross-host bytes, including M42→Spark→local and local→Ubuntu→M42. Counters are cumulative since fresh startup and include the small run/control traffic, not isolated large-run edge byte accounting.
Central-owned four agents stop with zero native stage children; the temporary M42 plan task is removed. Other Codex's Mac agent and unrelated apps are preserved.

### Preservation and next gate

Local bundle `F:/dev/p4-releases/all-hosts-retry-20260910-1436.zip` contains 154 raw/setup files plus MANIFEST.json; every listed ZIP member was read and SHA256-checked.
Archive SHA256: `8cbb915596b045fb329934008f7c9cd7177b3659c8005fe7091ab4c686a98dc6`.
Extract with `Expand-Archive -LiteralPath F:/dev/p4-releases/all-hosts-retry-20260910-1436.zip -DestinationPath <new-directory>`.
The large configuration, full artifacts, report, judgment, logs, mapped identities, memory samples, failed arms and helper command sources are retained under raw/. Setup includes the preceding Mac attempts and deployment records.
To reproduce after recreating the declared agents/endpoints, use the hash-bound p4-event-drive binary with the selected config.json and a new artifact output path; runtime archives remain in the earlier sealed candidate bundle. Do not reuse a live run's identities or mutate a measured directory.
This is durable local delivery only, not remote publication or another-machine retrieval acceptance. Generated raw logs and one-off tools are not added to Git in this retry.

Whole-fleet and CUDA/Metal acceptance remain BLOCKED. The next gate is an actual Mac agent reply after local authorization repair, then TUF SSH and Mac .20 NAS restoration, mixed small-model execution, requested M42/Spark/Mac122B, and all-host waves.
Transport write-error attribution/recovery, B2/B3, scientific response quality, synchronized cause tracing and sustained pressure remain open. A subset pass or agent restart does not close them.

Documentation checks for this retry: node tools/scripts/docs-lint.mjs exits0 with 92 files clean; git diff --check passes. No product test suite was rerun because only these evidence/roadmap/index files changed.

## Mac-included five-host retry: 2026-09-10

The user requested another run including Mac. Fresh P4 CREATE and DELETE both return ok=true with matching correlations from Mac .21 to Spark.
Mac agent PID78595 and SHA256 b7bb40ded261dbd6d213156d131cb6912644c75727427198f6d449653f4f3b6c are unchanged from the blocked run.
This turn changes no Mac permissions, signing, launcher, executable or firewall. The external cause of recovery is not established; actual executable traffic now passes.
The first Spark helper lookup failed and its probe saw a refused ingress; copying the known helper and starting a fresh agent resolves that setup error. An initial Ubuntu helper-path lookup also failed before the known helper was copied. These are preserved setup attempts, not failed model inference arms.

| Arm | Hosts/stages | Requested/completed/released | Result |
| --- | --- | --- | --- |
| `gemma-five-hosts-cuda-metal-1789022821079` | M42, Spark, Mac .21, local, Ubuntu / 5 | 8/8/8 | Exit0, all EOS, error/cleanup/missing null, UNLOAD pass; inference20,528ms |
| `122b-five-hosts-cuda-metal-1789022821180` | Same 5 physical hosts / 6 | 32/32/32 | Exit0, all EOS, error/cleanup/missing null, UNLOAD pass |

The small-model cuts are [0,5)/[5,7)/[7,9)/[9,13)/[13,35). All eight full responses were read; heating requests1/5 retain analogy/mechanism concerns, while RAM/storage, 84-litre arithmetic and calibration answers are coherent for these prompts.
There is no agent restart between passing small and large arms. Native no-alloc plans for new Spark/Mac cuts were inspected during the small functional gate; that arm is not used as a performance comparison.

The large model is the same three-shard Qwen3.5-122B-A10B UD-Q5_K_S, 88,310,156,320 bytes. Its original four prompts repeat eight times, four arrivals every500ms, resident4, max512, temperature0/seed7, no MTP/speculative execution.
The declared profile remains opt-in physical-wire-v4, with native upstream434ddbbc0 and patch ff1468f6. Product source9ad366f90 and all runtime files are unchanged.

| Stage | Host / backend | Owned layers | KV |
| --- | --- | --- | --- |
| 0 | M42 .29 RTX3090 CUDA0 | [0,8) | Kq8 / Vf16 |
| 1 | M42 .29 RTX3090 CUDA1 | [8,16) | Kq8 / Vf16 |
| 2 | Spark .26 GB10 CUDA0 | [16,29) | Kq8 / Vf16 |
| 3 | Mac .21 M4 Pro MTL0 | [29,37) | Kf16 / Vf16 |
| 4 | Local .6 RTX3090 CUDA0 | [37,45) | Kq8 / Vf16 |
| 5 | Ubuntu .19 RTX2070 Max-Q CUDA0 | [45,48) | Kq8 / Vf16 |

Artifact stage identities report the Mac inventory BLAS[BLAS]|CPU[CPU]|MTL[MTL0] and the other inventories CUDA; configured placement, actual allocations and native logs accompany the identity.
This is successful bounded CUDA/Metal execution for this model, layout and candidate. It is not generic cross-backend state compatibility, whole-fleet approval or H0–H7 completion. TUF and Mac .20 remain excluded.

Driver wall interval is 06:50:14.797Z–07:02:13.922Z. Inference elapsed283,612ms excludes LOAD/UNLOAD; 3,371 sampled outcomes include32 EOS, while decode rows are3,339: **11.7731 decode row/s**.
TTFT p50/p90 is115,987/218,911.7ms; completion p50/p90 is148,744/250,475.9ms, linear quantiles at(n-1)q. All32 incarnations are distinct; slots0/1/2/3 release9/8/8/7 times.
All32 full responses were read. Heating requests1/5/9/13/17/21/25/29 still equate electrical resistance with mechanical friction/rubbing hands. The remaining prompt groups are coherent at the tested level; whole scientific-quality and normal-service approval remain withheld.
The earlier four-CUDA run's15.2226 row/s and this run's11.7731 are different layouts/backends/KV configurations, single arms with background activity, not a controlled causal benchmark or an improvement claim. Neither is quality-approved effective TPS.

All six native no-alloc plans pass before large LOAD. Topology, execution shape and every host/device model/context/compute byte total equal ACTUAL for all six stages; current free memory and fits_current_free are not equality operands.
Mac requires device14,411,368,480B plus host14,696,480B; Spark device23,311,929,472B plus host14,958,624B. The unchanged CUDA cuts match their fresh plans as well.
Mac's model buffer is13,477.79MiB (about13.16GiB); the earlier commentary's13.48GiB conversion was explicitly corrected. Shared host/device capacities are not added as separate physical pools.
Runtime files mapped during LOAD match sealed identities: M42 stages9 each, local9, Spark7, Ubuntu7, Mac8. Mac native SHA256 is f17782080c1619595485241ecf2d2c9188d8791850e1a555d04b45ed8f54c8c2.
The driver hash c96cf7539af6580c3cc610939f1981477928aaa7415bc1c512e11172c097ed23 also matches after completion. NAS model hashes reference the earlier audit and are not a new88GB hash pass.

| Device | Samples in host enclosing RPC window | Mean | Zero samples | Startup-through-cleanup global VRAM peak |
| --- | --- | --- | --- | --- |
| M42 CUDA0 / CUDA1 | 243 each | 20.309% / 19.626% | 72 / 73 | 14,481MiB each |
| Spark GB10 | 272 | 9.794% | 119 | Unavailable |
| Local RTX3090 | 253 | 20.889% | 88 | 14,271MiB |
| Ubuntu RTX2070 Max-Q | 267 | 5.652% | 1 | 6,711MiB |
| Mac MTL0, AGX global Device Utilization | 262 | 62.378% | 23 | Shared-pool measurement; no comparable VRAM counter |

NVIDIA values are kernel-active samples. Mac values are the raw global Apple driver Device Utilization counter from ioreg; its semantics differ and include other applications. They are not summed, compared as equivalent occupancy, or labeled SM saturation.
Each window uses that host's own stage timestamps because clocks differ. RPC bounds are not GPU compute spans. Mac native RSS peaks14,673,904KiB; Windows native RSS remains missing. GPU/driver counters are not per-process isolation.
Actual sockets bind Spark→Mac and Mac→local to the Mac agent, with cross-host byte counters on Linux. Counters span reused connections including small/control traffic, not isolated per-large-run traffic volumes.
All five monitors exit0. After UNLOAD, Mac native PID84648 is absent, while its existing local-Codex-owned agent78595 is preserved. The four central-owned CUDA agents stop with zero native children; the temporary M42 planning task is removed. No preset H7 memory-return tolerance or sustained pressure arm was run.

Local bundle `F:/dev/p4-releases/mac-included-20260910-1602.zip` contains108 evidence/helper files plus MANIFEST.json, including both full artifacts, configuration, response/judge digests, source identities, native logs, memory samples, setup attempts and cleanup.
Archive SHA256: `dd4a02ed558dd8d83dd74807d34c47ac8bb84f50914668675e40ecbf42deb4bf`. Each manifest-listed ZIP member is read back and hash-verified at closure.
Extract with `Expand-Archive -LiteralPath F:/dev/p4-releases/mac-included-20260910-1602.zip -DestinationPath <new-directory>`; recreate declared endpoints with the sealed candidate runtimes, generate fresh run identities, then invoke the hash-bound p4-event-drive with config and a new artifact output path. One-off helpers are retained in raw/, not promoted to maintained tools.
This is local delivery only; remote durable publication/another-machine archive retrieval remains unapproved. Raw generated logs are not added to Git.
During the run, unrelated application/scaffolding changes appeared in the main checkout, including ignore rules; they were left untouched. The wire checkout0c27873ef and fleet documentation checkoutc5ad2e0a0 stayed clean before these closure edits; no product suite was rerun or prior totals relabeled.
The next fleet gate is TUF authentication and Mac .20 NAS restoration, then their legal cuts/plans and actual waves. Mac .21's old NECP failure remains historical evidence and is no longer an active blocker. Response quality, B2/B3, wait attribution and sustained service remain separate open work.

Closure checks for this Mac-included retry: node tools/scripts/docs-lint.mjs exits0 with92 files clean, git diff --check passes, and108 archived files pass SHA256 readback. Product tests were not rerun.
# MiMo 100k preflight (2026-09-10)

<a id="mimo-100k-preflight-2026-09-10"></a>

This addendum records a new failed preflight, not a revision of the earlier 122B acceptance.
The active experiment order and acceptance requirements are owned by roadmap §0.-5.

## Selection and executed scope

The model catalogue's largest successful actual LOAD is MiMo-V2.5 UD-Q5_K_S:
216,301,387,424 bytes across six shards, 51 declared blocks, 3 NextN blocks and 48 trunk layers.
Its previous 102,400-context/one-sequence load belongs to the older pin. Larger catalogue entries
without successful LOAD records were not promoted to supported models.

All five current hosts could read the six NAS shard files. With source `9ad366f90` and the previously
sealed runtime binaries, the following **inspect-memory-plan** consumers executed:

| Host/backend | Candidate trunk cut | Result |
| --- | --- | --- |
| M42 RTX3090 CUDA0 | [0,8) | exit7; no MEMORY_PLAN |
| M42 RTX3090 CUDA1 | [8,16) | exit7; no MEMORY_PLAN |
| Spark GB10 CUDA | [16,36) | exit7; no MEMORY_PLAN |
| Mac .21 M4 Pro Metal | [36,40) | exit7; no MEMORY_PLAN |
| Local RTX3090 CUDA | [40,46) | exit7; no MEMORY_PLAN |
| Ubuntu RTX2070 CUDA | [46,48) | exit7; no MEMORY_PLAN |

Each requested resident8, unified pool819200, per-session admission102400, batch512/UBATCH512,
no mmap, no speculative execution. All failed before allocation planning with:

```text
error loading model hyperparameters: key mimo2.attention.sliding_window_pattern has wrong array length; expected 48, got 51
```

The Windows runtime SHA256 is `98926adfb85c38decdd8438054cf4d234a142a149c7be753143cd3ac635f4825`;
Spark `f45e12d8f54f324d92b4455fe0e10c47d8b86d6d1fb208abf433effa9fb1dde0`;
Mac `f17782080c1619595485241ecf2d2c9188d8791850e1a555d04b45ed8f54c8c2`;
Ubuntu `83e3107e3adf098448eb3d937d7687f54d889fc76ca295db373fd8362726af2a`.
No actual LOAD, inference, KV placement, achievable concurrency, memory peak or TPS was obtained.
The failure is not an OOM result. M42 used its interactive scheduled-task route for S: access;
the task completed and was unregistered. No fleet inference agent was started or restarted;
the Mac agent owned by its local Codex was preserved.

## Diagnosis and unadopted repair

In the older prepared pin `0eadefebd3`, MiMo loaded its SWA array before reading NextN count.
In `434ddbbc0`, the common model loader reads NextN before `load_arch_hparams`;
MiMo's unchanged SWA reader therefore requests trunk48 against the declared51 array.
The proposed upstream change uses `n_layer_all` for that one array read. It preserves the 48-layer
execution trunk and does not trim GGUF metadata or weaken generic array validation.
It is **an unbuilt draft**, not a validated fix or deployed runtime.

The current `validateCompatibilityPatch` guard was exercised against the draft and rejected it:
`changes a model implementation; update official llama.cpp instead`.
The guard requires an official ggml-org PR URL and commit for `src/models/` changes. Neither was
invented; no upstream message/PR was sent and no guard was bypassed. Official-source adaptation
or explicit user authorization for a narrowly scoped local exception is required before adoption.
The remaining consumer, malformed-input, mutation and backend regressions are listed in the roadmap.

## Prepared workload, independently of inference

32 synthetic maintenance dossiers contain 179/361/520 distinct records each. Four queried records
span the beginning, middle and end. They require current/resistance/unit conversions, power and
energy arithmetic, pressure-threshold decisions, cross-record ranking, evidence citations and
a substantial engineering report. Expected numeric values are generated from the input facts,
not from a model response. This is a structured long-context workload, not broad domain-quality certification.

Using the current runtime's vocab-only tokenizer, all 32 prompts passed exact recounts:
31,645~92,165 input tokens; 1,972,902 tokens and 8,598,886 UTF-8 bytes in total.
Every prompt has its own content SHA256. The saved model template, rendered with Jinja2 3.1.6,
matches all32 prompts byte-for-byte for system+user, generation prompt on, thinking off.
Vocab-only success does not bypass or prove the failed model hyperparameter path.
Maximum requested output8192 plus the longest input is100357, within the102400 session bound.
The intended long-response gate is complete EOS, at least1024 generated tokens, independent
numeric/citation checks and full response review; it has **not run**.

## Preservation and reproduction

Local bundle `F:/dev/p4-releases/mimo-100k-preflight-20260910.zip` contains111 files plus
`MANIFEST.json`: six plan inputs/logs, preparation scripts, exact-token verifier, model template,
32 prompts/messages, numeric oracles, template parity, draft patch and task cleanup evidence.
ZIP SHA256: `9f007cb19e742c6960511098d43e59525930b2976884d9b759ae8f4202e96f16`.
It does not include the216GB model or replace binary provenance from the prior fleet bundle.
It is a local evidence archive, not cross-machine durable acceptance.

Extract the bundle into `target/mimo-100k-20260910` in the fleet checkout to reuse its relative
imports. `node target/mimo-100k-20260910/plan.mjs target/mimo-100k-20260910/mimo-100k-r8-plan-v1-local.json`
reproduces the local no-alloc RED with the saved runtime path and readable S: model share.
`workload.mjs` regenerates the dossiers and exact counts after building the existing tokenizer
probe; `template-check.py` validates the saved Jinja branch. The r16/r32 JSON files are unexecuted
planning candidates. The placeholder prompt in plan-only configs is not the sealed long-wave workload.

No product source changed. Rust/native regression suites were not rerun for this documentation-only
checkpoint, and historical counts are not reported as new results.

<a id="hy3-100k-2026-09-10"></a>

## Hy3 100k/session: selected fallback and OUTER timing (2026-09-10)

The user selected Hy3 after MiMo failed preflight. MiMo is held, not patched.
Hy3 Q5_K_S is206034401344 bytes (191.884 GiB),80 trunk layers plus one unowned NextN block.
Native/agent product source remains9ad366f90, upstream434ddbbc, physical-wire-v4.
Short gate used unchanged driver c96cf7539af6580c3cc610939f1981477928aaa7415bc1c512e11172c097ed23.

Run hy3-100k-smoke-lowport-1789027462065 passed2/2 EOS/completion/release/UNLOAD, no inference/cleanup error.
Six allocation plans match actual topology, shape and model/context/compute amounts; free memory is not compared.
Cuts and placement are in roadmap0.-6. Outputs correctly calculate28.8W/115.2Wh and18W/36Wh,
and withhold temperature conclusions for missing thermal conditions. This is short correctness only.
Mac accumulated logs are bounded by the latest P4_EVENT_AGENT_READY marker before this run is compared.

First LOAD hy3-100k-smoke-1789026789206 failed before artifact assembly with Mac ReadyFailed
Invalid argument(os error22). Native jobs still loading were stopped by verified owned PID/path.
After refreshing only dedicated validation agents, native ports changed53021/53022 to23021/23022
outside the observed ephemeral ranges. Collision is a candidate, not a proven cause. Mac GUI agent
was restarted for this test; the earlier claim of preserving PID78595 does not apply to this run.

### Approved OUTPUT receipt timestamps

OUTER RequestArtifact now serializes output_received_ms, parallel to approved outcomes. Time is captured
immediately after EventWire receives a complete frame and appended only after OUTPUT validation/commit.
Existing first_output_ms/completed_ms and TPS denominators stay unchanged. Gaps include transport batching
and OUTER scheduling; they are not GPU completion times. Partial failures keep approved timestamps.

RED: actual worker-captured OUTPUTs through EventWire/inference::drive,20ms paced delivery and40ms late
telemetry failed because the field was absent. GREEN:93 binary tests plus1 CLI test pass.
Full cargo test --workspace --no-fail-fast --locked exited0:1379 passed/0 failed/7 ignored,58 summaries.
Formatting-only follow-up reran the new consumer test successfully. Independent detached worktree
F:/dev/p4-hy3-timing-mutation-20260910, fresh target/timing-mutation, removed only append: test failed0-versus5.
Mutated inference.rs SHA256289938f4cac8baddd722a34a6aea5d99f3e8400ee3dbd291e2b9d487faa93fcd;
rebuilt test binary SHA2563ef94b19a0b276a084ea41811668dae417f06d96ea441a049591b7069edc92fb.

Active raw evidence is target/hy3-100k-20260910: RED/GREEN/mutation/full-suite logs, short config/artifact,
failed LOAD logs, plan outputs and workload/template checks. Full six-file model hashes are still being
collected. Long-wave verification and a local archive follow;100k inputs/eight active sessions/saturation
are not approved by this checkpoint.

### Hy3 long single result and diagnostic wave qualification

Long single hy3-100k-single-1789029008524 used OUTER source96e6fd1ae, driver
c6f8650a49c27764e88cc5ed311f77404c1c218f148d9d037b87133de12b1b76; native source remains9ad366f90.
It completed31643 prefill rows and2316 generated non-EOS tokens, then EOS/release/UNLOAD.
error/cleanup_error/evidence_missing are null. Inference elapsed2358643ms excludes LOAD/UNLOAD;
TTFT1162465ms, generation elapsed1196070ms. Actual generated-token receipt intervals, excluding the
empty terminal EOS frame: p50=513ms,p90=583.6ms,p99=691.6ms,max1399ms,2315 intervals.

Physical prefill is124 native-captured ubatches,123 at256 rows and one at155 rows. Logical prefill
calls are61 at512 and one at411 rows. Decode is2316 ubatches at one row each. Do not average these
phases and call low overall UBATCH fill the bottleneck. A logical issue may contain multiple physical
executions; the same-host RPC analysis deduplicates them by logical ordinal. With one request and
prefill_fragments default1, this run does not validate multi-flight saturation.

All six allocation plans match actuals. Mapped P4 files matched sealed identities: Spark7,Ubuntu7,
Mac8,local9,M42 GPU0 9/GPU1 9. Windows capture initially produced empty hashes because a child
Windows PowerShell could not autoload Get-FileHash; capture was corrected to streaming .NET SHA256
and rerun while the long job was alive. Empty maps were never approved. The earlier short gate has
no completed Windows mapped-module proof; its successful LOAD/runtime identity remains recorded.
Six full model hashes,206034401344 bytes, completed before the future wave. Single LOAD/prefill
partly overlapped that NAS hashing, so it is not a performance baseline.

**Full prose quality is not approved.** All four retrieved records, power/energy pairs, unit conversions,
pressure comparisons and energy ranking are correct. Response is1418 whitespace-counted words,
below the requested approximate1800-2500 target, although the unchanged1024-token runtime minimum passed.
Full reading found: alarm threshold called a safety limit; missing causal evidence described as
non-causality; a sentence saying confirmation requires invented data; an incomplete claimed duration
cycle. quality-review.json records quotes and reasons with passed=false. Do not cite raw row TPS as
quality-approved effective TPS.

The next16-request workload is explicitly a **diagnostic stress run**. It retains the same prompts,
including the prose-failing first case, and the same minimum1024-token/EOS/identity checks. Arithmetic
and transport passed; whole-response semantics remain independently graded, not weakened to pass.
The eight initial requests arrive at0/8/16/24/32/40/48/56 seconds; eight more at300..356 seconds.
Total input966748 tokens,4614390 bytes; resident8,context102400 each,maximum8192 output tokens each.
The fixed inference deadline is6 hours; observer cap8 hours includes LOAD and teardown margin.
The staggered arrivals offer independent ready cohorts while prefill_fragments stays at default1;
no outstanding/KV dependency guard is removed. This is not an optimality or service-latency claim.

The sealed upstream scheduler can offload operations on host-resident weights to CUDA for larger
batches; default GGML_OP_OFFLOAD_MIN_BATCH is32. RAM weight residence is not proof of CPU execution
for all phases. Source pointers and Windows CPU/GPU observations are in backend-placement-notes.txt.
The diagnostic wave adds NVML PCIe throughput sampling (API KB/s over its20ms counter window,
collected about once per second), retaining unsupported/error status rather than treating it as zero.
Device utilization is kernel-active sampling, not SM occupancy. M42 and Unix/Mac clocks differed by
about18 seconds in SSH-bounded probes; host-local RPC/monitor windows are used without assuming
cross-host alignment. Exact head-ledger flight time distribution remains uninstrumented; same-host
simultaneous distinct logical RPCs provide only a lower bound.
