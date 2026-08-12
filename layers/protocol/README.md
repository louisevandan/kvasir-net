# Protocol layer

| Path | Purpose |
| --- | --- |
| `src/contract/` | Message, phase, execution, error, and wire identity records. |
| `src/codec/` | Header framing, message kinds, bounded fields, payload encode/decode. |
| `src/catalog/` | Queue, terminal, correlation, and direction semantics. |
| `src/task/` | Self-describing transport-neutral task envelope. |

This crate is runtime-neutral and must not import Agent or adapter concepts.
