# Process entrypoints

`agent/`, `controller/`, and `node/` are thin launchers. They parse no backend configuration and contain no protocol, lifecycle, routing, or transport policy beyond calling the runtime's public entrypoints.
