# Running it

## An agent per machine

```bash
p4-agent 0.0.0.0:52001 192.168.0.6
```

An agent binds the `52001-52008` range because it stands where a node process
stood; the fleet's hosts already admit it, so there is no firewall to change.

The second argument is what this agent calls itself. Peers put it in an
envelope, so it must be the address they can reach rather than the interface it
bound. It takes either a host, which pairs with the bound port, or a full
`HOST:PORT` when the two differ. Omitted, it uses the bound address — fine on
one machine, wrong across a fleet.

On start it prints what it can serve:

```
P4_AGENT_READY address=tcp://192.168.0.6:52001 adapters=[llamacpp, mock, mock-instant, sglang, vllm]
```

An identity only this machine can reach says so, because the alternative is
diagnosing it from a chain that stalls after one token:

```
P4_AGENT_UNREACHABLE address=tcp://0.0.0.0:52001 peers=only-this-machine
```

### On macOS

The agent needs local-network access, and that grant is per binary rather than
inherited. Until it is given, the agent still accepts connections — listening is
not gated — so it takes work normally and its replies go nowhere. An agent that
receives requests while its caller reports no answer is showing this and not a
fault in P4: `consumed` and `forwarded` both rise while the machine has no
outbound socket to the caller.

Run it as a user LaunchAgent, which is what the node slots on these machines
already do. It has to be the GUI domain — a process detached with `nohup` has
no session that can raise the prompt, so it is refused with nothing to approve.

```xml
<!-- ~/Library/LaunchAgents/com.linkcpp.p4-agent.plist -->
<key>ProgramArguments</key>
<array>
  <string>/Users/USER/.local/share/linkcpp-p4/p4-agent</string>
  <string>0.0.0.0:52001</string>
  <string>ADVERTISED_HOST</string>
</array>
<key>RunAtLoad</key><true/>
<key>KeepAlive</key><true/>
<key>ProcessType</key><string>Interactive</string>
```

```bash
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.linkcpp.p4-agent.plist
```

The first outbound attempt puts `p4-agent` in System Settings → Privacy &
Security → Local Network, where it has to be switched on once. The grant then
survives restarts, including the ones `KeepAlive` performs.

It survives replacing the binary at that path: an upgrade installed over
`~/.local/share/linkcpp-p4/p4-agent` kept working without being approved again.

It does not extend to a copy somewhere else. A build run straight out of a
working tree was refused while the installed copy was serving normally at the
same moment — same source, same machine, different path. So a second agent
stood up for a test needs its own approval, and a macOS agent that takes work
and answers into nothing is worth checking here before anything in P4 is
suspected.

### On Linux

A systemd user service, with lingering so it does not need a login session:

```ini
# ~/.config/systemd/user/p4-agent.service
[Service]
ExecStart=%h/.local/share/linkcpp-p4/p4-agent 0.0.0.0:52001 ADVERTISED_HOST
Restart=always
RestartSec=2
[Install]
WantedBy=default.target
```

```bash
systemctl --user enable --now p4-agent.service
loginctl enable-linger "$USER"
```

Neither step needs root, and there is no permission to grant — the gate that
macOS applies has no counterpart here.

A node named against a backend not in that list is refused, because a placement
mistake is the caller's to fix and a silent fallback would hide it.

## Driving a fleet

```bash
p4-drive LISTEN CHAIN REQUESTS TOKENS [ADAPTER] [ADVERTISED]

p4-drive 0.0.0.0:52003 192.168.0.6:52002,192.168.0.29:52001 1000 64 mock 192.168.0.6:52003
```

`CHAIN` is the stage order, and `;` separates replica deployments:

```bash
p4-drive 0.0.0.0:52003 "A:52001,B:52001;C:52001,D:52001" 1000 64 mock HOST:52003
```

Requests go to the replicas in turn rather than filling one and moving on —
fed in blocks they would be one deployment working and one idle, which is what
running two drivers already measured. Replicas must have the same number of
stages, since `P4_DRIVE_SERVE` names stage indices and an index has to mean the
same thing in each.

Node names carry the replica only when there is more than one, so a single
chain names its nodes exactly as it always did. With replicas they must carry
it: two deployments can share an agent — a box with two cards is the obvious
case — and without the suffix the second `CreateNode` would name the node the
first one already made. `P4_DRIVE_PLAN_<d>_<n>` plans stage `n` of replica `d`,
because replicas are copies in shape and not in placement.

The driver creates a node per stage of each deployment, loads each, runs the
requests, and prints a verdict:

```
P4_DRIVE_RESULT requests=1000 tokens_each=64
  completed=1000 failed=0 unanswered=0 routes=1006
  tokens=63000 elapsed_ms=1211 frames_per_second=52846
  [pass] every request answered
  [pass] no request failed
  [pass] every stream in order
  [pass] one terminal per route
```

Throughput is reported but is not one of the claims. This layer does not own
throughput, and a figure from a simulated backend would say nothing about a
real one.

`ADVERTISED` is the driver's own identity, and it matters for the same reason
the agent's does: every reply is addressed to it. A driver naming itself by the
wildcard it bound asks a remote stage to answer its own loopback, so it says so
when the chain leaves this machine:

```
P4_DRIVE_UNREACHABLE address=tcp://0.0.0.0:52003 note=remote-stages-cannot-reply
```

`P4_DRIVE_CEILING` sets the concurrency each load declares; it defaults to 32.

`P4_DRIVE_KEEP_LOADED=1` is reserved for sustained-load or calibration runs. It
skips the driver's normal post-run unload so more requests can use the same
resident model; the caller must explicitly unload the deployment afterward.

The SSH-forwarded four-node runner also accepts `-Parallel`, `-PromptFile`, and
`-KeepLoaded`; use `-Parallel` for a calibrated `n_seq_max` sweep and keep the
same loaded deployment while changing only the request stream. Before copying
artifacts or opening a tunnel it verifies the local files and reserved ports,
the remote model, the remote artifact directory, and remote ports `52000`,
`53001`, and `53002`. A non-empty remote run directory or a port collision is
fatal; the runner does not stop pre-existing processes to make room.

The runner writes the complete driver result, including completed/failed/
unanswered requests, generated tokens, elapsed time, frame rate, queue peaks,
and sample count to `result.json`. It passes only when the ready marker and all
four driver verdicts are present, no quiet-timeout or unreachable marker was
printed, all requests completed with generated tokens, and the configured
overlap thresholds hold. Cleanup uses run-scoped PID files and leaves unrelated
processes alone. `-KeepLoaded` still preserves the driver's intentional
post-run unload skip; the caller owns the later unload.
On a successful `-KeepLoaded` run the runner also keeps its agents, SSH tunnel,
and copied remote artifacts alive so a follow-up drive can reuse the resident
deployment. A failed keep-loaded run is still cleaned up.

`P4_DRIVE_PROMPT_FILE` is what every request asks and `P4_DRIVE_OPTIONS` how it
is generated. A prompt sized in thousands of tokens belongs in a file, so what
was measured is exactly what was sent — `POST /tokenize` on the backend counts
it in the backend's own tokeniser, which is the only count that means anything.

`P4_DRIVE_ARRIVE_MS` spaces the arrivals. Zero — the default — sends the whole
run at once, which measures a backlog draining; a gap makes work arrive while
earlier work is still running, which is the case that never ends. With a gap
set, the driver also asks each agent what it is doing as it goes and reports
the peaks:

```
peak_node_queue=110 peak_in_adapter=10 peak_main_lane=0 samples=284
```

Read together, and only together. `peak_in_adapter` never above the declared
ceiling is the claim that P4 holds the excess rather than the backend — but it
is trivially true if nothing ever queued, which is what `peak_node_queue` is
there to rule out. `peak_main_lane` staying flat says the backlog lived on the
node rather than in front of it, and `samples` says somebody actually looked.

`P4_DRIVE_VARY` gives each request a prompt of its own, with a marker in front
because a prompt cache matches on the longest common prefix. Off by default,
and worth knowing which is being measured: one prompt sent many times measures
a cache as much as a model.

`P4_DRIVE_QUIET_MS` is how long nothing may arrive before the driver stops
waiting; 30s by default. It bounds silence, not duration: a five-thousand-token
answer takes as long as it takes. When the driver does stop, it says so above
the verdicts, because "we stopped watching" and "the deployment stopped working"
are different claims.

## Pointing a node at vLLM or SGLang

Registered under their own names against the same adapter today, which is a
transitional state rather than a design: each backend is getting its own
adapter, because two that look alike now diverge as their servers move. A plan
is the same shape and the chain is one link:

```bash
P4_DRIVE_PLAN=endpoint:127.0.0.1:8000 p4-drive 0.0.0.0:52000 127.0.0.1:52001 32 500 vllm 127.0.0.1:52000
```

vLLM matches the request's `model` against what it serves and answers 404 to
anything else, so a plan that names none is resolved from `/v1/models` at load
and the load fails if the server lists nothing. llama.cpp and SGLang answer to
any name and are not charged that round trip. Naming a model always wins: an
operator choosing one on a server holding several is making a placement
decision, and the adapter does not overrule it.

Neither vLLM nor SGLang has been run against here — neither installs on this
machine — so what is proved is the contract, against servers built to behave
like them, and not a particular release of either.

## Splitting one model across two GPUs

llama.cpp spreads a model over its own RPC backend, unpatched. Start a worker
per extra device and one server that reaches them:

```bash
ggml-rpc-server -H 127.0.0.1 -p 50052 -d CUDA1
```

```bash
llama-server -m MODEL.gguf --rpc 127.0.0.1:50052 -dev CUDA0,RPC0 -ngl 999 -ts 8,24 -c 81920 --parallel 64 -b 4096 -ub 512 --port 18090
```

`-b` has to hold more than one prompt or llama.cpp admits prefills one at a
time whatever P4 offers it: at `-b 256` against 389-token prompts, slots filled
one every 17-20 seconds. `-ub 1024` crashes in `ggml-cuda` with an illegal
memory access, so 512 is the ceiling worth using.

`-ts` is the ratio the shares are split in, and `--list-devices` after `--rpc`
names the RPC device so `-dev` can pin what the server itself uses — without
that it would take the second card twice, once directly and once through its
worker.

P4 gives each share a node, so a placement is stated rather than implied. The
plans differ per stage, and only the front serves:

```bash
export P4_DRIVE_PLAN_0='{"role":"worker","device":"CUDA1","vram_gb":23,"endpoint":"127.0.0.1:50052"}'
```

```bash
export P4_DRIVE_PLAN_1='{"role":"front","device":"CUDA0","vram_gb":11,"endpoint":"127.0.0.1:18090","workers":["127.0.0.1:50052"]}'
```

```bash
P4_DRIVE_SERVE=1 p4-drive 0.0.0.0:52000 127.0.0.1:52001,127.0.0.1:52001 16 96 llamacpp 127.0.0.1:52000
```

`P4_DRIVE_SERVE` names the stages an inference visits — here stage 1 only,
because stage 0 holds a share and has no completions surface. It must end at
the last stage, which is the one created as the deployment's tail. Both stages
still load, and both must bind.

The run prints one answer beside the verdicts. That is deliberate: four passes
are equally consistent with every token being empty, which a real backend has
produced twice.

## Making the network bad on purpose

```bash
p4-link LISTEN TARGET [--delay MS] [--jitter MS] [--rate BYTES_PER_SEC] \
                      [--stall-every N] [--stall MS]

p4-link 0.0.0.0:52005 127.0.0.1:52011 --delay 35 --jitter 15
```

A relay that carries frames badly. The agent binds somewhere private, the relay
takes the port peers use, and the agent advertises the relay's address — which
is what an agent behind any gateway already does, so nothing in P4 changes or
is told.

Bytes are never dropped or reordered; TCP does neither, and a relay that did
would be testing a transport this layer does not have. Everything is
deterministic, jitter included, so two runs of one scenario differ only if P4
differs.

## Watching an agent

```bash
P4_AGENT_STATS=1 p4-agent 0.0.0.0:52001
```

```
P4_AGENT_DEPTH control=0 prefill=812 decode=44 response=0 nodes=196
P4_AGENT_TRAFFIC forwarded=38409 consumed=3 to_nodes=38403 unrouted=0 peers=3 waiting=0
P4_AGENT_NODE node=tail-0 received=12801 queued=12801 claimed=12800 hops=745 …
```

Read the first two numbers together. A deep node queue beside shallow lanes
puts a slowdown below the adapter; deep lanes beside an idle node put it here.
The per-node counts exist because a frame that goes missing leaves no trace in
a depth reading — depth only shows what is still waiting.

## Which backends a build carries

Registered in [`entrypoints/agent/src/adapters`](../entrypoints/agent/src/adapters).
`mock` reproduces the measured shape — cost by chain position, prefill dearer
than a lap — so a fleet can be loaded anywhere. `mock-instant` answers with no
delay, for proving routing and ordering at rates a timed backend would hide.

A node's name tells a staged backend which position it plays: `stage-N` is an
intermediate stage, `tail-N` is the end of a chain, and any other name is a
backend that spreads the model itself.
