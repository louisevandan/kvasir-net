# Responsibilities and wiring

`entrypoints/agent` factory → `HfNodeAdapter: RetainedNodeAdapter` in `adapter/` → bounded IPC → per-model Python worker.
Nodes talk to each other through the P4 event/broker. The common core does not interpret model payloads.
Rust does not reimplement the Python OUTER controller's model scheduling or the per-stage computation and cache.
The bridge owns capacity, result attribution, output approval and process lifetime.
To swap the Python bundle, the same agent runs NODE_UNLOAD and then NODE_LOAD with a new generation.
Wire format and ownership follow the [integration contract](integration/README.md); code roles follow the [structure](structure/README.md).
