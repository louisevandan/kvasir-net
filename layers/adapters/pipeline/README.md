# Pipeline adapter

Concrete adapter for the Linker Pipeline runtime. The name states which backend
this crate serves; the adapter interface itself is the P4 message contract and
is not a crate.

| Path | Purpose |
| --- | --- |
| `src/application/lifecycle/` | Health, load/draft, and unload use cases. |
| `src/application/execution/` | Batched and persistent-stream inference paths. |
| `src/domain/` | Capability and binding state. |
| `src/infrastructure/` | Listener, HTTP, and Agent-local transport annotation. |
