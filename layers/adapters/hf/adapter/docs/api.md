# api

HfNodeAdapter::new(endpoint,input_capacity,completion_capacity,retained_capacity,retained_bytes) is consumed as Arc<dyn RetainedNodeAdapter>. The public content types are COMMAND/RESULT.

The [integration specification](../../docs/integration/README.md) owns the wire, budget and deployment rules and the error semantics.
