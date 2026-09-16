# Folder ownership by role

User-confirmed principle: **if roles differ, split them into separate folders, even for a single file.**
Model separation and role separation apply together; files are never mixed into one folder with only different names.
Multiple implementation files within one role may sit together. Language and file count are not the criteria for a role boundary.

## Implemented structure

| Folder | Single owned role |
| --- | --- |
| `python/p4hfadapter/transport/framing/layout/` | Fixed byte layout of the wire header |
| `python/p4hfadapter/transport/framing/limits/` | Explicit frame size limits |
| `python/p4hfadapter/transport/framing/errors/` | Classification of rejection and transport errors |
| `python/p4hfadapter/transport/framing/encoding/` | Send-side header validation and serialization |
| `python/p4hfadapter/transport/framing/decoding/` | Receive-side header parsing and validation |
| `python/p4hfadapter/transport/framing/receiving/` | Blocking frame receive; no reuse after a receive failure |
| `python/p4hfadapter/transport/framing/sending/` | Blocking frame send; no retry of an uncertain transmission |
| `tests/transport/framing/<role>/` | Verification of that role; `process/` holds real inter-process communication tests |
| `tests/fixtures/framing_peer/` | Transport-only subprocess fixture |
| `tests/fixtures/receiving_streams/` | Streams for injecting receive errors |
| `tests/fixtures/sending_streams/` | Streams for injecting send errors |
| `scripts/testing/` | Test suite entry point |
| `scripts/verification/framing_mutation/` | Framing mutation verification in an independent copy |
| `scripts/verification/documents/` | Document encoding and link checks |
| `tests/plans/`, `tests/reports/framing/` | Verification intent and actual run evidence, respectively |

## Model implementation structure

Qwen's actual role list is in the implementation table of the [model contract](../models/qwen3_5_0_8b/README.md).
Process lifetime (`processes/`) and node traversal (`routing/`) are also split into separate folders.

`python/p4hfadapter/models/<model>/` is the ownership boundary of a concrete model and does not mix the roles below.

```text
models/<model>/
  configuration/   validation of the selected model's execution specification
  loading/         loading of the assigned weights and metadata
  planning/        selection of measured stage candidates for this model, and resource totals
  profiling/       measurement of the real loader and cache in a new process
  quantization/    this model's recipe application and kernel selection
  forward/         this model's partial computation
  state/           this model's per-request KV/recurrent state
  boundary/        this model's stage input/output representation
  reference/       comparison baseline that preserves the vendor execution
```

Write real code starting from the roles that are needed; do not create empty skeleton folders in advance.
Add a new role as a new folder. Do not gather different responsibilities into `common`/`utils`/`runtime`.
The Rust bridge likewise splits retained delivery, ledger and IPC into per-role subfolders.
Per-role folders do not require a common model base class or an automatic plugin structure.

Only `/models/` is ignored by Git, as the storage location for large weights. The model-specific source in
`python/p4hfadapter/models/` is tracked. Tests, fixtures and measurement results do not go into production folders.
