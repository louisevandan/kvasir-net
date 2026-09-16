# Process entrypoints

> Document status (2026-09-06): **Component guide**. This is an API and structure guide for this path. The legacy service path and the current event path are distinguished by their actual callers.
> Current goals, status and ordering follow the [execution roadmap](../docs/distributed-batching-roadmap.md); document authority and reading paths follow the [document map](../docs/document-map.md).

`agent/`, `controller/`, and `node/` are thin launchers. They parse no backend configuration and contain no protocol, lifecycle, routing, or transport policy beyond calling the runtime's public entrypoints.
