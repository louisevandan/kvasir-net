# P4 implementation layers

| Layer | Dependency direction | Contract |
| --- | --- | --- |
| `protocol/` | depends on nothing P4-specific | Wire records, codec, semantic catalog, task envelope. |
| `runtime/` | depends on `protocol/` | Agent policy, routing, task dispatch, transport abstractions. |
| `adapters/` | depends on `protocol/`; concrete runtime APIs | Backend translation only; never defines P4 policy. |

Dependencies point downward to `protocol/`; the protocol never imports runtime or adapter code.
