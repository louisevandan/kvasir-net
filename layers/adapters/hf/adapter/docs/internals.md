# internals

Both an in-flight Input and a held Completion keep their claim. The completion is reserved before the worker runs, and work resumes through the capacity listener. The lifetimes of the façade and the child are kept separate.

The [integration specification](../../docs/integration/README.md) owns the wire, budget and deployment rules and the error semantics.
