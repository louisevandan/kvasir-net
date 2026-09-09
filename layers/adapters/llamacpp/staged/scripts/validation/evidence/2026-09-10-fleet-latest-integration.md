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
| This PC, `.6` | RTX 3090 24 GiB + RTX 4080 16 GiB; 255.9 GiB host RAM | `S:` maps to `//192.168.0.13/file-station` | New CUDA build in progress; no fleet model acceptance yet |
| Spark, `.26` | GB10; 121.6 GiB shared system memory | `/mnt/file-station/models`, 156 GGUF files; CIFS source verified | SSH/CUDA 13.0/Cargo available |
| Mac mini, `.20` | M4 Pro, 64 GiB unified memory | Existing placeholder contains zero models; no SMB mount | SSH/Metal tools/Cargo available; NAS authentication needs user connection |
| Mac mini, `.21` | M4 Pro, 64 GiB unified memory | `/Volumes/file-station/models`, 156 GGUF files | SSH/Metal tools/Cargo available; existing LM Studio left running |
| Ubuntu laptop, `.19` | RTX 2070 Max-Q 8 GiB; about 30.7 GiB host RAM | `/mnt/nas-file-station/models`, 156 GGUF files; path resolves into the logged-in user's GVFS SMB mount | SSH/CUDA 12.4 available; Git/Cargo absent from probed paths |
| TUF laptop, `.17` | Not verified in this run | Not verified | TCP 22 reachable; SSH key rejected; access information requested |
| M42-SERVER2, `.29` | Prior release evidence: RTX 3090 x2; refresh before new LOAD | Interactive `42mob` login/S: recovery pending | SSH restored after planned Windows Update reboot; release agent/stage remain stopped |

Spark and Mac GPU memory are the same physical pool as system memory. They must not be added a second time.
The `.21` working repository is dirty and was not changed. New builds use isolated directories.

Port probes found 19001 listening only on this PC among the hosts tested. Its health response identifies an existing Linker service; it is not a free P4 port.
Local Windows firewall inspection found a TCP 52001-52005 rule. Both Mac application firewalls report disabled.
Linux firewall inspection requires root and remains unverified. A closed TCP probe does not distinguish a firewall from an absent listener.
Peer-to-peer connections and actual P4 delivery remain to be tested.

Mac `.20` normal SMB mount and NetFS NoUI attempts failed authentication. The matching NAS Keychain item exists, but Security.framework returned `-25293` when asked to use it without user interaction. No password was logged or transferred. User connection through Finder was requested.

## Compatibility replay

Plain replay of the old 26-patch queue failed on 0001, 0015, 0016, 0018 and 0020.
The candidate queue has 25 patches and replays on a fresh official tree; `prepare-pipeline-upstream.mjs` passed its patch, runtime-boundary and prepared-tree checks.

| Identity | Value |
| --- | --- |
| Candidate patch set SHA256 | `a22eed7eea2c49ea30f97fc353193d1eb3c24b7e6461514619ed6c3b73103675` |
| Candidate patched tree | `d07413951cb24a696324f1fe34483a7887573460` |

0001 is a candidate retirement: upstream `992cb503cdacf691ef06c332d05243bc7807257b` removed the old split-capacity check and uses dynamically growing split inputs. Runtime proof of the old multi-input boundary remains required before adoption.

0020's remaining rejected hunk was the recurrent rollback test context helper. Upstream added a `fill` parameter and poisoned-buffer/nonfinite-logit checks. The port retains those checks, adds `n_ubatch` as a fourth parameter, and passes `fill` into the added regression contexts. No expected result or tolerance was relaxed. The runtime regression has not yet run against this candidate.

Builds, native conformance, individual normal responses, large model cuts/placement, all-host distributed delivery and sustained waves are pending. Replay success alone is not update success.

## Local evidence and reproduction

Raw discovery, NAS inventories, compiler versions, conflicts and rebase logs are under `F:/dev/p4/target/fleet-20260910`.
These ignored files are local evidence, not a portable publication bundle.

```powershell
node layers/adapters/llamacpp/staged/scripts/prepare-pipeline-upstream.mjs --json --out F:/dev/p4-fleet-20260910/target/prepared-latest
node layers/adapters/llamacpp/staged/scripts/build-stage-server.mjs --cuda --cuda-architectures '86;89' --generator Ninja --config Release --parallel 8 --build-dir F:/dev/p4-fleet-20260910/target/native-latest-cuda
```

The upstream checkout must be clean at the full candidate SHA before preparation. The release checkout and its prepared source are not inputs to these commands.
