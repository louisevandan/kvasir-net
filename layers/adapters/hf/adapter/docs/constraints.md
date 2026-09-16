# constraints

Model class, tensors and KV are not interpreted in Rust. Partial I/O is uncertain and is not retried automatically. The Python environment is not advertised as ready until LOAD readiness.

The [integration specification](../../docs/integration/README.md) owns the wire, budget and deployment rules and the error semantics.
