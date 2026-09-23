# Patches to the p4 engine

These apply to the p4 engine source (`p4-kvasir-src`), not to anything in this
repository. They live here because the engine tree on the fleet hosts is an
unpacked source drop with no version control, so a change made there is lost
the next time the drop is replaced. Apply from the engine root:

```sh
cd <p4-kvasir-src>
patch -p0 entrypoints/agent/src/event_runtime/transport/journal.rs \
  < .../0001-agent-journal-occupancy-cache.diff
cargo test --release -p p4-agent     # needs node on PATH for the docs gate
cargo build --release --bin p4-agent
```

## 0001 — the agent journal walked its whole directory before every write

`Journal::require_capacity` ran before every journal record and called
`occupied_bytes`, which did a `read_dir` of the journal root plus an `lstat` on
every entry. A journal never removes anything, so that walk grew with every
record the agent had ever written, and each write got slower than the last.

It is the single largest cost in the ring. Measured on Step-3.7-Flash across
two MI250 stages, decoding 100 tokens per run:

| | run 1 | run 2 | run 3 | run 4 | run 5 |
| --- | --- | --- | --- | --- | --- |
| before | 22.90 | 18.82 | 15.53 | 12.58 | — |
| after | 30.04 | 31.21 | 28.21 | 29.04 | 27.84 |

Splitting the same four stages across two agents made it far worse, because
every agent-to-agent hop writes journal records that a single-agent ring never
writes: 4.10 tok/s on a fresh journal, 0.75 tok/s four runs later. That decay
is what a day-old two-host ring had reached when it was serving at roughly
1.5 seconds per token, and it was misread at the time as the cost of the
cross-host hop. The host boundary was not the cause.

The proof is a controlled one. Padding the journal directory with 20,000
zero-byte `.admission` entries — which charge exactly nothing against the byte
budget — took decode from 9.52 to 2.23 tok/s, and deleting them put it back to
7.71. Against the patched agent the same 20,000 entries cost nothing at all:
28.45 → 28.80 → 29.83. So the cost was the directory walk, not the bytes, and
not the fsync either: moving the journal to tmpfs, where fsync is free but
`read_dir` is not, recovered only about 10%.

The patch keeps an occupancy figure per journal root and charges each write
against it, walking the directory only at startup and again whenever the
estimate approaches half the cap — where the exact number is what decides
whether a write is refused. The estimate only ever over-counts, so it can
refuse a write early but never admit one that does not fit. The figure is
shared by every handle open on a root, which `concurrent_bounded_handles_admit_only_one_record`
checks; one agent holds a root exclusively, so no other process writes behind
the count.

This does not address the other half of the problem: nothing in the journal is
ever retired, so it still grows without bound and still costs disk. Retiring
settled records is the follow-up.

## Related: the stage servers spin

Not a patch — a plan setting, in `p4bridge/make-plan.mjs`. All the arithmetic
runs on the GCD, but each stage server starts a 48-thread ggml CPU pool and
libgomp's default wait policy spins those threads while idle, so two stages peg
all 96 logical cores and the agent's per-event work runs on scraps. The stage
servers were burning about 790 CPU-seconds each per 100-token run. The plan now
sets `OMP_WAIT_POLICY=PASSIVE` and `GOMP_SPINCOUNT=0`, which cut that to about
190 and took agent CPU per token from 116 ms to 8 ms.
