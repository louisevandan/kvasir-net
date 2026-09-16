# Model-free cluster return-route verification plan

2026-09-15. User-specified scope: query the specs and node lists of every cluster we own, and relay to the MI250 over SSH.
The [explicit return context](../../docs/event-protocol-v2.md#required-request-return-context) contract owns this.

## Environment and assumptions

- Windows local, m42 and TUF; Linux Spark and Ubuntu; mac20 and mac21; two external MI250: 9 physical hosts.
- A test agent identical to source `dde0813fb`. It uses a different directory and port from the existing installed app.
- Do not create or load a model. The test agent's nodes must be an empty list from start to finish.
- The LAN uses TCP41997. The MI250s go through an Ubuntu SSH jump and use forward and reverse loopback tunnels that the local host maintains.
- SSH credentials use the existing local keys. Never copy key contents or put them into evidence.

## Procedure and expected values

1. Survey the existing installed addresses and SSH access separately. Do not treat a failure on one port as a failure of the whole installed app.
2. Pin the Git archive and the per-platform binary SHA256. On POSIX, build with `cargo build --release --locked -p p4-agent --features hf-transformers`.
3. Pass a `{name, host, port, address}` array to the [runner](../../tools/cluster-envelope-check.py). Keep the dial address distinct from the P4 advertised address.
4. Check direct queries, 3 runs of every ingress×target combination, cross requests at 3× the target list, concurrent generation-1/2 requests on the same channel, and a generation-3 reconnect.
5. Every response's target/return_route must equal the original OUTER, and source must be the queried target. Also cross-check correlation/causation, the specs and the empty node list. Never reuse an event ID across different requests.
6. Inject absent/mismatch return contexts over real TCP, confirm the EOF/reset rejection, then run a normal query again. A timeout is not a successful rejection.
7. Run LAN 7×7 and local gateway/MI-A/MI-B 3×3 separately. This is not a single 9×9 mesh test.
8. Preserve the results, raw failures, stderr and the source/binary hashes. After the full test, confirm nodes=[] with a direct query, and clean up only the owned PIDs, tasks, firewall rules and SSH tunnels.

## Evidence and scope

Command: `python tools/cluster-envelope-check.py CONFIG OUTPUT`.
Raw data: `target/cluster-envelope-20260915/`. The report links the committed summary seal and the run results.
The model-free query verifies the default event runtime return. It does not extend to re-verifying node-derived events, per-model mixed owners or create/cancel, nor to performance acceptance of large models.
Settlement of unknown outcomes on a dropped connection, the reconnect policy and the existing FINISH failure are separate remaining items.
