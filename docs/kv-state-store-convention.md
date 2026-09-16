# KV persisted-state store convention

> Document status (2026-09-06): **domain contract; distinct from implementation**. Read the contracts/targets of the owned domain, but do not treat them as implemented. If this conflicts with the current development order, follow the explicit hand-over in the roadmap.
> For current goals, status and order see the [execution roadmap](distributed-batching-roadmap.md); for document authority and reading paths see the [document map](document-map.md).

Reflects the 2026-08-31 contract-correction round. This document is the **sole owner** of storage organization, record identity,
session concurrency and 2PC convergence rules. The implementation already has the file *format* (.lkv
magic/version/identity/SHA-256/atomic publish), but
any contract below that is not yet in the code is marked in its section, and the implementation phases are owned by
the K branch of the [distributed batching roadmap](distributed-batching-roadmap.md).
P0/P2/P3 and similar labels in the body are old backlog identifiers; links to the new phases follow the roadmap.

## Current state and defects

Current: flat layout `<kv_root>/<cache_key>.lkv`; `cache_key` is the adapter's copy of
`cache.sequence`; verification is only an in-file comparison at load time.

| Defect | Consequence |
| --- | --- |
| save overwrites without reading the existing file's identity | silent destruction on key collision, discovered only at load |
| no model or stage scope in the file name | multiple nodes on the same volume destroy each other's files |
| no time fields at all | no basis for idle TTL decisions |
| no enumeration structure | GC, quota and idle scans impossible |
| record is a single 128MB file | sessions above ~55K tokens per node cannot be stored |
| content of `model_identity` unspecified | just a string, no convention |
| receipts are also flat: `<kv_root>/.p4-transactions/<operation_id>.receipt`, no stage scope in the path | on a shared volume, 4 stages collide on the same operation_id |
| Prepare writes only a receipt and creates no durable staged copy (`transaction_store.cpp::TransactionStore::prepare` @ df5b9ce7) | no basis for recovery if something is lost during commit |
| locks are per operation_id (`TransactionStore::Lease`) | Persist/Restore/Discard/GC on the same session can run concurrently |
| adapter Reconcile collapses `Committing` into `Inconsistent` | one real receipt state is left entirely undefined (`server.cpp::Session::handle` @ df5b9ce7 performs commit as Committing durable record → side effect → Committed; the definition is `protocol.hpp::KvReceiptState`) |

## Storage unit: what one record is

**Shard record = (base_model_id × cut_id × kv_format) × (session_key ×
snapshot_key × kv_variant_id × position)** — one session can have several
persisted footprints under different keys.

The load ID (`load_generation`) is not record identity. A node that reloads the same model with the same
layer configuration reuses existing records. The generation is bound
only to operation coordination, through `Cache.generation`.

## Three grades of identity

| Grade | Items | On mismatch |
| --- | --- | --- |
| Layout identity (engraved in the path) | base_model_id, layer range [b,e), K/V types, v_trans/flash, memory family, **state_abi_id** | different record. Not reusable |
| Acceptance conditions (checked on restore) | position < n_ctx_seq, free cells, free slots | reject but keep the record |
| Reference info (recorded only) | build_identity, n_batch/n_ubatch/n_seq_max at save time | warning log |

A grade downgrade applies only to items that pass the verification matrix of plan P2; until they pass,
the current exact match (fail-closed) stays in force.

Build provenance and state compatibility are separate axes — using the whole patch-set hash as
identity would repeat D6, where a build that changed one logging line kills every persisted
record. Split it into four.

- `build_id` (reference info): upstream commit + patch_set_sha256 + backend
  provenance. It serves diagnostics and evidence; it is not identity.
- `state_abi_id` (layout identity, included in path derivation): the compatibility generation of sequence state
  serialization. **Owned by the compat manifest**; arbitrary manual input is
  forbidden. The actual state bytes are produced by `state_write/state_read` of each
  upstream memory family under `llama_state_seq_get_data_ext()`, so **an upstream pull can change the format
  even when the P4 patches do not change.** Therefore every pin must pass
  the state compatibility gate: per memory family, compare a (pin N-1 writer →
  pin N reader) restore against an (N writer → N reader) reference run — state
  byte structure, position and subsequent logits/tokens must match. On failure, bumping
  state_abi_id in the manifest is mandatory. The gate runs with a small fixed fixture model and
  per-family golden states (repository test assets).
- `backend_layout_id` (record level, recorded in meta and checked on restore): a normalized fingerprint of the
  device/buffer-type/placement the state actually used. llama.cpp
  allows multiple devices, CPU fallback and per-tensor buffer overrides even within one run,
  so a single `backend_family` value cannot express it.
- Restore allowed = `state_abi_id match ∧ (source_layout → target_layout) passes the P2
  verification matrix`. Cross-layout portability is not a public llama.cpp contract, so
  it stays fail-closed until a round trip proves it.

The `cut_id` path component is derived from layout identity only — if build_id were mixed in,
the same over-pinning would recur in the path.

## Model identity: base_model_id × kv_variant_id

A single model_id cannot identify every artifact that affects KV —
for LoRA the scale changes KV in addition to the digest, for a control vector the scale and layer range change
KV, and llama.cpp can change a context's LoRA set and scales at runtime.
Split it in two.

- `base_model_id` (path component, full 64 hex): SHA-256 of all bytes of the base GGUF
  (for a split GGUF, the SHA-256 of the list formed by concatenating each part's 32-byte
  digest in ascending `-%05d-of-%05d` part index order). Tokenizer identity is guaranteed by the base file
  digest. A summary of the header, tensor table and sizes can be identical even when tensor data differs,
  so it cannot serve as identity.
- `kv_variant_id` (record level, recorded in meta and checked on restore): SHA-256 over the
  canonical binary encoding of the auxiliary artifacts that affect KV.
  v1 = `(version u32, count u32, entry*)`,
  entry = `(role u8 ∈ {mmproj, lora, control_vector}, digest 32B,
  scale f32-bits u32, layer_begin i32, layer_end i32)`,
  sorted by role, then digest (order-independent canonical form). In deployments that allow dynamic LoRA changes,
  kv_variant_id is **session/record identity**, not load identity.
- Verification policy: full recomputation on every load is the default. A size+mtime sidecar is not proof of
  content, so it cannot replace recomputation on the production path; only a content-addressed manifest whose
  full hashes were verified at ingestion can replace it.
- Negative tests (P-1 pass condition): 1-byte tensor tampering in base (on both paths: no cache and a tampered
  sidecar), LoRA scale change, mmproj tampering, control-vector range
  change → restore rejected; input with only the entry order changed → same id.
## session_key contract (sk-v1)

- The format is **mandatory**: `sk1:<owner>/<conversation>`.
  - The length limit of 1..512 bytes applies to the **whole raw key**, including the `sk1:` prefix.
  - The whole key must be valid UTF-8, and C0/C1 control characters are forbidden.
  - The separator is the single **first `/`** after the prefix. Any additional
    `/` inside `<conversation>` is treated as data.
  - `<owner>` and `<conversation>` are each at least 1 byte and cannot consist only of whitespace
    (at least 1 non-whitespace code point).
  - **No Unicode normalization is performed.** Comparison and hashing always use byte identity, and
    two keys that differ only in NFC/NFD are different keys. If normalization is needed, OUTER does it
    before building the key.
  - Format violations are rejected before path computation.
- `<owner>` is a stable namespace owned by the OUTER deployment, and `<conversation>` is
  a stable conversation ID within it (an OUTER-generated random ID is recommended). The namespace is
  mandatory because different conversations that use the same raw key become the same record, and raw
  byte comparison cannot detect that semantic collision. Guaranteeing uniqueness is
  OUTER's duty; the adapter can only enforce the format.
- Path component: `sk-v1/<sha256(raw key bytes) full 64 hex>` — the contract version is
  in the path. Contract changes happen only through an `sk-v2` path.
- `meta.json` stores the raw key and the full digest, and restore requires raw key byte
  equality + a full comparison of base_model_id, kv_variant_id, cut_id and kv_format beforehand.
  The uniqueness scope is the `(base_model_id × cut_id)` tree — the same key under a different model or a different
  cut is a different record.
- Negative tests: invalid UTF-8 / empty key / 513 bytes (including prefix) / no `sk1:` /
  no separator / whitespace-only owner or conversation → rejected; confirm that a key with `/` in the conversation
  is accepted and split only at the first `/`; two keys differing only in NFC/NFD
  → different records; same `<conversation>` with a different `<owner>` → different records;
  digest/raw mismatch in meta → restore rejected.

## Record bundle and atomicity

state, tokens and meta together form one record. So that partial combinations (new KV + old
tokens, etc.) cannot exist, use **immutable generation directories + an atomic pointer**.

```
<kv_root>/v2/<base_model_id 64hex>/<cut_id>/
  sessions/sk-v1/<sk-digest 64hex>/
    snap-v1/<snapshot_key digest 64hex>/   # named footprint. Key grammar and path
                            # digest rules reuse the session_key contract
      gen-<n>/              # immutable bundle. Files inside must not change after creation
      state.lkv             # (state-<k>.part above 128MB, open item)
      tokens.bin            # all prompt+generated token IDs, as LE i32 values
      meta.json             # position, record_generation=n, SHA-256 of state and of
                            # tokens, raw session_key, base_model_id,
                            # kv_variant_id,
                            # cut_id, kv_format, raw snapshot_key, saved_at
      MANIFEST              # per-snapshot atomic pointer: {generation, meta_sha256}
    CONTROL                 # session authority epoch/generation, atomic replace
    ACCESS                  # advisory last access time {last_access, epoch}, atomic replace
    lease.json              # single-writer lease for the session shard
  receipts/<operation_id>.receipt
  tmp/                      # same volume. Guarantees atomic rename
```

- Publish order: complete and fsync `gen-<n>` → atomically replace `MANIFEST`
  (CAS on `(expected_epoch, expected_generation)`) → GC old generations. Whenever
  a crash happens, MANIFEST points only to complete bundles.
- The receipt binds `(record_generation, position, state_sha256, tokens_sha256)`.
  The binding between LCP evidence and KV position is thus proven at the receipt level.
- The final LCP judgement is a byte comparison of `tokens.bin` (70K tokens ≈ 280KB). A chain
  digest is only an optional speed-up. Tokenizer binding is guaranteed by base_model_id.

## kv_root topology

This axis is a premise of the lock, rename and telemetry contracts, so it is stated explicitly.

- **Default (recommended): node-local disk.** CONTROL/lock mutual exclusion holds through the OS atomicity of a single
  host (exclusive create), and boot_id/pid liveness can be judged
  locally, which removes the stale-authority problem. cut_id path separation exists
  for deployments where several stages live on one machine (the current local 4-node setup).
  In no topology does the coordinator read store files directly —
  its input is always wire telemetry.
- **Shared volume (SMB/NFS, etc.): conditionally supported.** It requires both a storage capability
  gate that proves exclusive create, replace rename, flush and lock visibility on that volume,
  and a membership authority such as a durable heartbeat or an external lease service.
  Without them it is unsupported and the load is rejected.
- P0 failure tests include "partition → stale-break → old writer returns".

## Session shard serialization

- The serialization unit is not the operation but **(base_model_id, cut_id, sk-digest)**.
- The authoritative epoch is owned not by the lease but by the durable `CONTROL = {epoch, generation}`
  record. **rename alone is not CAS** — two competitors can read the same
  epoch and each succeed at an atomic replace. A CONTROL update runs
  read → compare with the expected value → write tmp and fsync → rename inside a critical section made mutually exclusive
  by exclusive create of `control.lock` (POSIX `O_CREAT|O_EXCL`, Windows
  `CREATE_NEW`). The lock file holds `{host_instance_id, boot_id, pid, operation_id, acquired_at}`
  — on a shared volume pids repeat across hosts, and remote process liveness
  cannot be judged by a local pid check, so host_instance_id (a unique random value generated at node
  install) and boot_id (renewed on every boot) identify the owner. A stale lock is broken only after
  confirming that the owning node is gone or has rebooted, and only together with a CONTROL epoch check
  (crash recovery contract).
- Each lock gets a random `lock_token`, and release deletes the lock file **only when the current lock file's
  token is my token** — this prevents an old owner that returns after a stale-break from
  deleting the new owner's lock.
- **Authority for stale judgement**: owner identification says only who took the lock, not whether the owner died
  or is partitioned. `acquired_at` cannot be the sole authority because of host clock
  skew. The authority is set by the deployment topology (see
  kv_root topology below), and automatic break is forbidden when the authority is uncertain.
- Layering principle: **locks are for liveness (less contention and duplicate work); safety is
  guaranteed by store_epoch binding.** Granting the CONTROL epoch itself, however, requires real
  mutual exclusion, so deployments where that mutual exclusion does not hold are not
  supported (load rejected). Lease acquisition holds only after epoch+1 has been durably recorded
  in this critical section, after which `lease.json`
  `{operation_id, kind, epoch, acquired_at}` is written. Deleting and recreating the lease alone
  cannot stop a late publish by the previous owner.
- **Comparison and publish form one critical section.** MANIFEST replacement, staged promotion and receipt
  finalize run `read CONTROL → compare (expected_epoch, expected_generation) →
  publish` inside the same session lock. If only the comparison is inside the lock and
  publish is outside it, a race remains in which a new writer bumps the epoch in between and the old writer's
  rename still succeeds. A generation-only CAS cannot stop a stale writer after a new
  lease.
- P0 failure tests include "the publish of an old writer that returns late after a stale-break is
  rejected".
- Persist, Restore, Discard, TrimTo, GC and ACCESS updates proceed only under a lease.
- A multi-shard operation acquires all cut leases of the session in **ascending stage_index** order,
  and if any acquisition fails it releases everything acquired and retries — the total order of
  acquisition prevents deadlock. "One coordinator per session" is a premise derived from OUTER's single-authority
  principle, and the line of defence when it is violated is this lock order plus epoch
  fencing.
- Terminology: epoch in this document means the session store fencing epoch (`store_epoch`).
  It is a different concept from `stream_epoch` in pipeline fragment identity (plan P4.5),
  and the two do not share a name.
- `record_generation` increases monotonically, and every publish is a CAS on the expected generation.
  A Persist at a lower position that finishes late is discarded by CAS failure —
  "supersede" is not a rule but the result of this CAS.
- GC skips sessions whose lease it fails to acquire.
- Required failure tests (P2): Persist↔Restore, Persist↔Discard, Restore↔GC,
  double Persist — all are serialized or fail cleanly, with 0 cross-corruption.

## LCP Trim barrier

Truncating a branched prompt is a distributed change across 4 stages.
`TrimTo(session, position, tokens_sha256_at_position, epoch)` is defined as a 2PC
operation: prepare on all stages (verify target position and digest) →
commit (each runs `seq_rm [position, ∞)`) → suffix prefill starts **only after all stages attest**.
Prefill while only some stages are truncated produces silent
wrong answers, so it is forbidden. TrimTo is issued only under the session lease and epoch, and it
records (position, tokens_sha256, epoch) in the receipt; convergence of a partial commit is
owned by the TrimTo row (roll-forward) of the 2PC table. The implementation phase is owned by plan
P3.

TrimTo is not an operation that every memory family supports. Recurrent state cannot
be cut at an arbitrary suffix (`llama-memory-recurrent.cpp::seq_rm` @ upstream d7a20741 —
"can't have a state partially erased at the end"), and partial rollback succeeds only for a single use within the retained
snapshot depth, returning false outside it.
So each stage reports `trim_support = arbitrary | bounded:<depth> | none` as a
capability, and TrimTo prepare attests each stage's trim_support and remaining
rollback range. If any stage cannot handle the target position,
TrimTo is not issued and the request is **downgraded to a full re-prefill**.

A persisted record can be ahead of the resident state (record position > last truncation
position), and `tokens.bin` can be read without importing state. So the restore
decision is completed **before import**:

1. After verifying meta and tokens (checksums, identity comparison), compute the LCP against the request prompt
2. LCP = record position (exact prefix) → reserve cells → Restore → suffix prefill
3. LCP < record position (branch) ∧ all stages can trim → reserve → Restore
   → TrimTo(LCP) → suffix prefill
4. branch ∧ any stage cannot trim → **skip Restore entirely** and do a full
   re-prefill — this removes both the waste of importing a dead suffix only to erase it and the partial-failure
   path
5. On any path, never start decode on top of a restored suffix without an LCP decision

## 2PC convergence rules

**P2 implementation requirement (not a premise):** PreparePersist must create a durable staged bundle.
The current prepare writes only a receipt (`transaction_store.cpp::TransactionStore::prepare`
@ df5b9ce7). Until this requirement is implemented, the Persist recovery guarantees below do not
hold.

The receipt lifecycle is `Prepared → Committing(durable) → [side effect] → Committed(durable)`
(the KvCommit branch of `server.cpp::Session::handle` @ df5b9ce7; states are defined in
`protocol.hpp::KvReceiptState`). A stop before, during or after the side effect is therefore always observed as
`Committing`, and **whether the side effect completed must be judged from durable evidence**.
The current behaviour, in which adapter Reconcile collapses Committing into Inconsistent, is
replaced by this judgement in P2.

| Operation | Observation (including restart) | Judgement evidence | Convergence |
| --- | --- | --- | --- |
| Persist | all prepared, every stage attests a resident of the same epoch | resident attest ×N | rollback → all-resident (Abort, discard staged) |
| Persist | all prepared, but resident attest failed (including restart — resident is volatile) | staged copy valid | **roll-forward → all-persisted**: after a restart, Abort cannot revive the resident |
| Persist | prepared, resident lost + staged invalid/absent | — | Inconsistent (stop) → downgrade to re-prefill |
| Persist | committed ≥1 + prepared remainder | staged copy exists | **roll-forward → all-persisted** (retry the remaining Commits; committed cannot be revived by Abort) |
| Persist | Committing, final bundle valid | bundle checksum matches | roll-forward: redo finalize (idempotent) |
| Persist | Committing, no final, staged valid | staged checksum | roll-forward: re-promote, then finalize |
| Persist | Committing, both staged and final invalid | — | Inconsistent (stop). Unreachable if the staged requirement is met |
| Restore | all prepared | record intact | rollback → all-persisted |
| Restore | committed ≥1 or Committing | import is volatile, record intact | **rollback → all-persisted** (withdraw with `seq_rm` on importing stages, then retry everything) |
| Discard | committed ≥1 or Committing | whether the bundle remains | roll-forward → absent (if it remains, redo the deletion, finalize) |
| TrimTo | prepared only (no stage truncated) | — | rollback (Abort) |
| TrimTo | partial commit (`seq_rm` on some stages only) | (position, tokens digest, epoch) in the receipt | **roll-forward**: retry truncation at the same position on the remaining stages (idempotent). TrimTo is issued only after the coordinator has confirmed discarding the branch suffix, so only moving forward is safe |
| All operations | manifest/checksum mismatch | — | Inconsistent: stop, no automatic retry, mark the record as discarded, downgrade to re-prefill |

The current core coordinator (`layers/service/src/cache.rs::recover` @ df5b9ce7,
test `recovered_partial_restore_is_failed_closed`) folds a partial Restore, and a committed
stage found during Aborting, straight into failure — that test pins the current behaviour;
it is not an implementation of this table. The paths where Persist rolls forward and Restore rolls back
are added as backend-neutral core fixes with no llama knowledge (plan principle 1).

## Snapshot command model (direction settled 2026-08-31)

The trigger for persistence is not a timeout alone. In branching agent workloads,
"persist the existing KV and branch the tree into a copied new session" is an everyday
operation (LM Studio mlx-engine agentic workloads, and the Context Checkpoints of llama.cpp/LM Studio,
are precedents in the same direction), and **policy — when, what and why — is
owned entirely by OUTER**. Adapters and nodes never persist or unload on their own;
they only carry out the command vocabulary below. TTL, quota and branch timing are all policies that OUTER
expresses in this vocabulary.

| OUTER command | `CacheAction` mapping | Status |
| --- | --- | --- |
| Persist session S under key K and **release the resident** | `Persist` (+2PC Prepare) | exists — needs a fix so cache_key becomes an OUTER-specified snapshot key instead of a copy of the sequence (`cache_direct.inc.rs::cache_key` @ 87ec1317) |
| Persist session S under key K but **keep it resident** (leave a footprint and continue) | **new `Checkpoint` needed** | missing — the current `Persist` comment argues that "a persist that keeps the state resident releases nothing, so it is one verb" (`work/cache/mod.rs::CacheAction::Persist` @ 87ec1317), but that argument did not consider the branch-footprint use case |
| List the persisted keys of session S | **new `SnapshotList` needed** | missing — `Reconcile` is a single-operation receipt query. The unit of intersection is not the key but the **logical snapshot** — all fields of `{snapshot ref generation, operation_id, position, tokens_digest, state_abi_id, kv_variant_id}` must match. A field mismatch or partial presence is Inconsistent (unusable) |
| **Load session T** from key K of session S (disk branch) | extend `Restore` with a target | partial — the current Restore is "into the same id". The target's resident state must be empty, and the restore decision ladder applies as is to T's request prompt |
| Immediate branch of a resident session (memory copy) | `Fork { into }` | **exists** — the "copies rather than aliases" contract as is |
| Unload a session (release without persisting) | existing release path | exists |
| Discard key K | `Discard` (+2PC) | exists |

- `Persist`/`Checkpoint` are implemented not as separate state machines but as the single shape
  `Snapshot { after_commit: KeepResident | ReleaseResident }` — identical through durable publish,
  differing only in the postcondition (accepting the 8th review's recommendation;
  minimizes the llama.cpp update surface).
- Snapshots are **immutable**. Re-persisting under the same key goes into that key's gen-N
  chain (supersede = CAS inside the key). Different keys never replace each other.
- A disk branch (RestoreInto) does not copy the record — conditionally:
  ① it reads from the same storage domain, ② cut and identity compatibility hold, and ③ it breaks the relation to the source **only after
  import completes on all stages**. Discarding the source before that
  must be blocked by a read-pin (O10), and node moves or cross-domain cases use the copy
  path. After that, persistence of T writes into T's session directory.
- Batch coupling: Checkpoint, Persist and Fork run only when the target sequence is at a **stop point** (no in-flight
  rows, all stages settled). The fence and insertion schedule are owned by the
  batching contract ([adapter-batching-layers.md](adapter-batching-layers.md)
  invariant 11).
- Session lease, CONTROL and epoch stay at session level — different
  snapshot operations on the same session are also serialized (simplicity first; if a bottleneck is measured, refine to
  per-key granularity at that point).
### Storage tier

Snapshot commands take a `tier` argument — the destination is not only disk.

| tier | Medium | Survivability | Use |
| --- | --- | --- | --- |
| durable | SSD — the full file machinery of this convention applies | survives process exit and reboot | long-term footprints, session moves |
| ram | host CPU memory — file machinery does not apply, kept in-process | **volatile**: gone when the process ends | fair swap under overload: move some sessions' KV down to RAM, serve other requests, then reload |
| resident | checkpoint inside the backend (VRAM) | context lifetime | fast rollback and branching (O7, after capability negotiation) |

- ram tier: keeps `llama_state_seq` bytes in host memory instead of a file
  — llama-server's `--cache-ram`/idle-slot offload is a precedent in the same direction.
  Crash convergence is **Absent, not Inconsistent**: loss in a volatile tier is
  absence, not corruption. It does not survive restarts, so it carries no cross-pin state ABI
  burden either. Receipts record the tier, and a receipt pointing to a ram record
  resolves to Absent after a restart.
- ram bytes are an acceptance-accounting axis separate from GPU cells (host bytes;
  reported through the MEMORY_ACTUAL host entry and telemetry). Together with the durable disk byte
  budget, it is owned by O11.
- Swap policy consists entirely of OUTER commands: swap out =
  `Snapshot{tier=ram, ReleaseResident}`, swap in = `Restore(ram)`. The batch
  consistency fence (invariant 11) applies in the same way.
- This makes `max_resident` logically exceedable — the GPU cell cap becomes
  a cap on "concurrently resident" sessions rather than on "live" sessions, and the trade-off between swap round-trip
  cost and TTFT impact belongs to OUTER policy.

## Reservation 2PC

For multi-node cell reservation and multi-shard lease acquisition, "release everything on failure" is
a statement that holds only while the network is healthy. Reservation is therefore defined as 2PC.

- Prepare: `(reservation_id, session_key, requested_cells, ttl)`.
  The TTL is evaluated not as absolute time but on **each node's local monotonic clock, starting when that node
  received Prepare** — host clock skew is excluded from authority.
- All stages Prepared → Commit. Abort/Release are idempotent.
- **Prepared reservations are included in acceptance accounting too**, and are visible to the coordinator through the telemetry field `reserved_cells`
  — if they were invisible, over-admit would recur through the reservation path.
- A coordinator restart converges pending reservations via Reconcile; on a communication break,
  each node reclaims them automatically by its local TTL.
- Failure tests: partial prepare, lost release, coordinator death — in every
  case 0 reservations are left behind (TTL reclaim confirmed).

## Lifetime rules

- Access time is held in an `ACCESS` file outside the immutable bundle (atomic replace, monotonic
  maximum, advisory). Keeping it inside the immutable `meta.json` would either break immutability on every access or
  mix access accounting with content generations. GC reads ACCESS and, if it is corrupt, treats the entry as
  the oldest. However, ACCESS lives on **node-local disk**, so
  the coordinator's victim selection input is not a file read but
  the per-session `{last_access, position, bytes}` telemetry that the adapter
  sends up over the wire (the same channel as D12's occupancy
  report). The ACCESS file is merely local persistence of that telemetry so it survives restarts.
- Remove `tmp/` leftovers at boot. Orphan `gen-*` directories not proven by MANIFEST or a valid
  receipt are **quarantined or GC'd without being exposed** — reviving an unpublished generation
  by re-indexing is forbidden.
- Delete old generations after a new publish succeeds (= the MANIFEST CAS succeeds).
- The adapter **never creates or deletes any snapshot on its own** — every trigger, including TTL
  decisions, is an OUTER command.
- Deleting an exposed record is always a **4-stage Discard 2PC issued by the coordinator**. A session record is a set of shards across
  several cuts, so if each node deleted independently based only on ACCESS
  it would create a partial-absent state. TTL and quota victim
  selection is owned by the coordinator, and ACCESS is one of its inputs.
- Local GC is limited to cleaning up orphan generations that MANIFEST does not expose and tmp
  leftovers.

## Remaining open items

- Boundary and checksum rules for `state-<k>.part` in bundles above 128MB.
- Proof matrix for the items whose acceptance-condition grade is to be downgraded (owned by plan P2).
- Accepting auxiliary state for DENIED memory families comes only after a 3-axis audit (including backend conformance)
  passes.
