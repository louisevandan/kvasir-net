# Recovering one stage's agent on the GB10 ring

**Written for:** whoever next has to restart a p4 agent on the GB10 pair — an
operator, or a session on #1, #2 or the supervising machine.

**Basis:** the recovery of GB10 #2 on 2026-09-25, 06:22:58Z–06:27:10Z. Every
number below was measured during that run. Anything that was not measured says
so. The procedure was written afterwards, not before: it had been run once when
this was written, and the step that nearly went wrong (5a) is in here because it
did nearly go wrong.

**Outcome of that run:** `serving:false` for about four minutes, a real request
answered afterwards, and no socket left behind on either agent.

## When you need this

An agent has to be restarted when it stops accepting connections. The known
cause is the slot leak reported in
[DEFECT-agent-slot-leak.md](./DEFECT-agent-slot-leak.md): a direct-mode
connection keeps its semaphore permit after input EOF, even once the peer is
gone, and the agent stops accepting once 256 have leaked. The signs:

- a new connection to `:42011` sits in the LISTEN Recv-Q and the agent logs no
  `CONNECTION_OPENED`
- `inspect.mjs` or `load.mjs` hangs
- the gauge shows `agent_sockfd` near 256, or `hidden` rising

The ring keeps serving on the bridge's existing connections, which is why this
goes unnoticed. **Do not restart the bridge before the agent is fixed.** If the
bridge loses its connection to a wedged agent, it cannot get it back.

**A restart kills that agent's stage**, because the agent is the stage process's
parent. So this is always a full ring reload: both stages end up at a new load
generation.

## Who does what

The ring is driven from **#1**: `load.mjs` speaks p4 to both agents directly, so
no ssh between machines is needed. The machine whose agent is being restarted
(here #2) only stops the agent, moves the journal, starts the agent, and
measures.

| Step | Machine | Action |
| --- | --- | --- |
| 0 | both | record baselines |
| 1–4 | broken machine | stop the agent, confirm the stage is gone, move the journal, start the agent |
| 5a–5c | #1 | unload the surviving stage, check memory, generate a plan, load both |
| 6 | #1 | restart the bridge, send a real request, run the checks |

**Approval:** moving a journal is the owner's decision — see step 3. Everything
else follows from that approval.

## Step 0: baselines, before touching anything

Without a "before" there is no way to judge the "after". This costs a minute.

On **#1**:

- `/api/runtime` → `dial_ledger` for both agents. Before the run both read
  `slots_spent 1 · unproductive 0 · dials_last_hour 0 · connected true`.
- the last line of the gauge log: `agent_sockfd`, `unique`, `hidden`
- stage pid (`pgrep -af 'p4_staged_server --port 42100'`), `MemFree`, `Buffers`
- `state/last-load.json` — it holds the current generation

On the **broken machine**: agent pid *and cmdline*, stage pid, GPU memory,
`MemFree+Buffers`, and the gauge values.

## Steps 1–4: restart the broken agent

### 1. Stop the agent

```sh
kill -TERM <agent pid>      # after checking the pid AND its cmdline
```

Check the cmdline, not just the pid. A path pattern can match a process in a
mirror directory instead of the one you mean.

- **Measured on #2:** under `TERM` the stage went down with the agent — no
  orphan, GPU memory dropped by 28.4 GiB, port 42100 freed, one second from
  signal to exit (06:22:58Z → 06:22:59Z).
- The mechanism is the agent's own shutdown, not the cgroup. #2 reported both
  processes sitting in `system.slice/ssh.service`, so killing one pid was never
  going to take the other by cgroup membership. **Not independently checked.**
- **Not measured:** a forced kill (`-KILL`) skips that shutdown and may leave
  the stage orphaned, holding GPU memory and port 42100 — which would then make
  the new agent's stage load fail on a port conflict.
- **Whichever you use, measure afterwards.** No `p4_staged_server` left, `:42100`
  not listening, GPU memory released. If a stage is orphaned, stop it on its own
  and measure again.

**Measured effect on the other machine:** #1's peer connection from #2 ended
with `CONNECTION_STOPPED … unexpected end of file` plus `HOP_CONTROL_CLOSED`.
That is the hop path, so the slot was released and #1's `hidden` did not rise.
The bridge began redialing: 93 s after the kill its guard read `slots_spent 2 ·
unproductive 1 · failed_probes 2 · next_dial 12s`.

### 2. Confirm the stage is gone

As in step 1. Do not take it on trust.

### 3. Move the journal — never delete it

```sh
mv ~/p4-journal ~/p4-journal.gen-<old generation>.$(date -u +%Y%m%dT%H%M%SZ)
mkdir -m 775 ~/p4-journal
```

Then check the moved file count (26,510 on 2026-09-25) and that owner and mode
match the old directory (`drwxrwxr-x tony:tony 775` on #2). **Not measured:**
what the agent does with a journal directory it cannot write. Keep `agent.log`
too, renamed by the old pid.

> **A precondition that can never be satisfied.** The agent unit's header says
> not to move a journal whose census shows request incarnations or provisional
> submissions. On a ring that has served traffic, finished requests stay
> counted, so **that condition never holds** (measured 2026-09-23). Read
> literally, the guidance forbids every recovery.
>
> This move went ahead on the owner's direct approval, not on a clean census.
> Say so when you do the same. The alternative — quietly deciding the rule does
> not apply — is how a rule that was protecting something real gets dropped
> without anyone noticing.
>
> **Measured once:** after the move, the restarted agent issued its stage
> normally and refused nothing. One clean run is not proof the census was
> guarding nothing. It is one data point, and it is the only one there is.
> This belongs back with the agent's authors as a question: what was the census
> condition for, and what is the right check on a ring that has served traffic?

### 4. Start the agent

Exactly as it is normally started — #2 uses `run-agent.sh`, #1 uses
`systemctl --user start kvasir-agent`.

- **Pass condition:** `P4_EVENT_AGENT_READY` appears, **and** a new agent with no
  clients holds **4 socket fds** (3 unique: listener + 2 unix, plus 1 dup).
  `AGENT_READY` alone is not a pass — it says the process started, not that it
  started clean.
- **Measured:** one second from the start command to READY.
- **Stop** if READY does not appear, or if the socket count is well above 4.
- Signal #1 with the agent pid, the socket count, and `MemFree+Buffers`.

## Step 5: reload the ring, from #1

### 5a. Unload the surviving stage — and only that stage

**Why this step exists at all.** Step 1 took down only the broken machine's
stage. The other one is still loaded at the old generation and holds about
89 GiB. A new stage needs at least 88.1 GiB free, so it cannot load until the
old one is out. This step was missing from the first version of the plan and
would have stopped the recovery at 5c.

**Why the record is narrowed.** `load.mjs --unload` sends UNLOAD to every stage
named in `state/last-load.json` and then waits up to 300 s for *all* of them to
answer. The restarted agent no longer has its stage, so it answers failed or not
at all; the command errors, the surviving stage's result becomes ambiguous, and
the catalog is not cleared. Narrowing the record makes the command's scope match
what actually exists.

```sh
cd ~/kvasir-s5/p4bridge
PRE=state/last-load.json.pre-recovery-$(date -u +%Y%m%dT%H%M%SZ)
mv state/last-load.json "$PRE"
# keep only the stage that is still loaded, at the old generation
python3 -c "
import json,sys
d=json.load(open(sys.argv[1]))
d['stages']=[s for s in d['stages'] if s['node']=='step37-s0']
json.dump(d,open('state/last-load.json','w'),indent=2)
" "$PRE"
P4_BRIDGE_CATALOG=~/.local/share/kvasir-bridge/catalog.json node load.mjs --unload --dry-run
P4_BRIDGE_CATALOG=~/.local/share/kvasir-bridge/catalog.json node load.mjs --unload --confirm
```

Move the record, never delete it. Losing it can strand the model: the load
generation is chosen by whoever loads and is not in any snapshot, and without
it the stages refuse every session and cannot even be unloaded (see the header
of `load.mjs`). **Not measured:** whether it can be recovered from the node
`generation` in a snapshot, which `load.mjs` forces to equal the load
generation (`load.mjs:207-216`).

**Measured:** `unloaded: step37-s0` · `catalog: load generation cleared`, in 2 s.

### 5b. Check that the memory came back

First confirm with `ps` that the old stage's pid is gone and `:42100` is no
longer listening. Then read `/proc/meminfo`.

- **Pass condition: `MemFree + Buffers ≥ 88.10 GiB`.** Page cache does not count.
- **Measured:** 106.7 + 1.74 = 108.4 GiB, so no cache drop was needed.
- If it is below, drop the GGUF from the page cache
  (`os.posix_fadvise(fd, 0, 0, POSIX_FADV_DONTNEED)` on the model file; no root
  needed) and measure again.
- **If it is still below 88.1, stop.** The process is gone but its memory did not
  come back, and loading now would OOM the node.
- The other machine needs room for its own stage: #2 read 102.2 GiB against the
  28.7 GiB its stage needs.

### 5c. Generate a plan and load both stages

```sh
python3 make-ring-plan.py --site sites/gb10.json \
  --record state/last-load.json.pre-recovery-<timestamp> \
  > plan.json
```

- **Pass `--record` explicitly**, pointing at a record that holds the *old*
  generation: the one moved aside in 5a, or a dated backup.
- **If stderr says `warning: could not read …`, stop.** That warning means the
  check that the clock has not gone backwards did not run. The generator warns
  and continues by design — a corrupt bookkeeping file should not block a
  recovery — which is exactly why the warning must not be skimmed past during
  one.
- Check by eye that the new generation is greater than the old.
  **Measured:** 1790317534470 > 1790181517946.
- Before commit `6d8cc82`, `--catalog` crashed right after an unload, because
  the unload writes `null` into the catalog's generation. With an older copy,
  pass only `--record`.

```sh
P4_BRIDGE_CATALOG=~/.local/share/kvasir-bridge/catalog.json node load.mjs --plan plan.json --dry-run
P4_BRIDGE_CATALOG=~/.local/share/kvasir-bridge/catalog.json node load.mjs --plan plan.json --confirm
```

- **Measured:** `loaded: step37-s1, step37-s0` in 25 s; the catalog updated to the
  new generation, and `load.mjs` rewrote `state/last-load.json` as the full
  two-stage record.
- **If it ends `failed`, do not retry — stop**, and have the broken machine send
  its `agent.log` verbatim. Retrying at a generation the stages disagree about
  is how a model gets stranded.
- **Measured:** the new agent did *not* refuse to issue the stage after the
  journal move. That was the first confirmation of it.
- **Tool check, free of charge:** on each agent every `load.mjs` connection
  logged `CONNECTION_OPENED` then `CONNECTION_FINISH`. `--dry-run` opens no
  connection at all, so it cannot be used as a connection test.

## Step 6: restart the bridge. This is part of the recovery.

```sh
# on #1 the user bus is not inherited; export these first or the command fails
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
systemctl --user restart kvasir-bridge
```

**Why it is required.** While the agent was down, the bridge's dial guard backed
off after each failed dial — 60, 120, 240, 480, 900, 1800 s — and caps dials at
six an hour. A restart clears that state.

On 2026-09-25 the bridge had already reconnected on its own: there was a single
failure, so the backoff was 60 s, shorter than the recovery took. That is why
the shutdown log read `finished` for both agents rather than `destroyed`. **The
reason to restart is not that the bridge cannot reconnect. It is that when it
reconnects should not depend on how long the recovery took.** A longer recovery
would have left it waiting up to half an hour.

If the old connection to the restarted agent was already dead, `destroyed (…)`
in the shutdown log is expected and is not a failure.

**Pass conditions — all of them. "It came up" is not a pass.**

| Check | Measured 2026-09-25 |
| --- | --- |
| `/health` 200, `serving_models` 1, `inspect_error` null | 13 s after the restart |
| a real request, with enough `max_tokens` to finish | `"42"`, finish `eos`, 167 tokens, stage_rows 167 / 167, 15.1 s |
| restarted agent: `agent_sockfd` / `unique` / `hidden` | 8 / 7 / **0** |
| other agent: back to its step-0 count, `hidden` unchanged | 13 → 13 fds, hidden 0 |
| `transport.failures` | #1 1 (unchanged) · #2 0 (new agent) |
| `dial_ledger` reset | both `spent 1 · unproductive 0 · connected true` |

A request with `max_tokens 16` ends in `length` inside the model's reasoning.
That proves the pipeline carries tokens; it does not prove an answer.

**If `serving:true` does not appear, stop** and send the full `dial_ledger`.

## Stop conditions, collected

- **1–2:** a stage is orphaned and does not go when stopped on its own
- **4:** no `AGENT_READY`, or a new agent's socket count is well above 4
- **5b:** `MemFree+Buffers` still below 88.1 GiB after the cache drop
- **5c:** the generator warns it could not read a record; the new generation is
  not greater than the old; or the load ends `failed`
- **6:** no `serving:true`; the real request fails; `hidden` above 0 on either
  agent; or the other agent's socket count has grown

At any stop: **change nothing more**, and report with the verbatim log lines. The
journal and the old record were moved rather than deleted, so both can be put
back.

## Reading the gauge

| Field | Meaning |
| --- | --- |
| `agent_sockfd` | socket fds the agent holds. WARN at ≥128, CRIT at ≥192 |
| `unique` | distinct socket inodes — fds can be dups of one socket (#1 has 2, #2 has 1) |
| `hidden` | unique socket inodes in **no** `/proc/net/{tcp,tcp6,udp,udp6,unix}` table |

**`hidden` is the leak itself.** It should be 0. If it rises, something is still
leaking — and it shows there long before the total approaches a threshold.

Two earlier methods give wrong answers and both were believed for a while:

- **CLOSE-WAIT** misses every leaked socket that received an RST. It read 4 on an
  agent that was completely full. A whole day of "we have plenty of room" came
  from this number.
- **`fds − sockets visible to ss`** counts dup fds as leaks, so it is non-zero on
  a healthy agent, and the dup count differs per machine — a fixed correction
  does not generalise.
