# Repository guidance

## Versioning

The unit and runtime-pack release version is defined once in the root `VERSION` file.

- Increase the patch version (`0.0.x`) for every important user-visible or
  runtime-affecting change.
- Important changes include proxy-mode changes, merging or materially changing
  all-node/distributed-node behavior, protocol or runtime compatibility
  changes, planner/resource-allocation changes, and Docker/runtime deployment
  changes.
- Keep `controller/release.py` as the sole loader for `VERSION`; consumers
  such as `controller/protocol.py`, `controller/versioning.py`, the hub UI,
  Dockerfiles, and version assertions must derive from that single source.
- Do not release a runtime-changing change with an unchanged unit version.
